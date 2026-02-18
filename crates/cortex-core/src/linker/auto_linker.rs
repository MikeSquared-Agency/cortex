use crate::error::Result;
use crate::graph::GraphEngine;
use crate::linker::{
    AutoLinkerConfig, AutoLinkerMetrics, ContradictionDetector, DecayEngine, DedupScanner,
    LinkRule, ProposedEdge, SimilarityLinkRule, StructuralRule,
};
use crate::storage::{NodeFilter, Storage};
use crate::types::{EdgeProvenance, Node, NodeId, Relation};
use crate::vector::{embedding_input, EmbeddingService, VectorIndex};
use chrono::{DateTime, Utc};
use std::sync::{Arc, RwLock};
use std::time::Instant;

const CURSOR_KEY: &str = "auto_linker_cursor";
const CYCLE_COUNT_KEY: &str = "auto_linker_cycle_count";

/// Auto-linker: Background process for self-growing graph
pub struct AutoLinker<S: Storage, E: EmbeddingService, V: VectorIndex, G: GraphEngine> {
    storage: Arc<S>,
    graph_engine: Arc<G>,
    vector_index: Arc<RwLock<V>>,
    embedding_service: Arc<E>,
    config: AutoLinkerConfig,
    decay_engine: DecayEngine<S>,
    metrics: AutoLinkerMetrics,
    cursor: DateTime<Utc>,
    cycle_count: u64,
    /// Pre-allocated structural rules (avoids re-creation per node pair)
    structural_rules: Vec<StructuralRule>,
    /// Pre-allocated similarity rule
    similarity_rule: SimilarityLinkRule,
    /// Pre-allocated contradiction detector
    contradiction_detector: ContradictionDetector,
}

impl<S: Storage, E: EmbeddingService, V: VectorIndex, G: GraphEngine> AutoLinker<S, E, V, G> {
    pub fn new(
        storage: Arc<S>,
        graph_engine: Arc<G>,
        vector_index: Arc<RwLock<V>>,
        embedding_service: Arc<E>,
        config: AutoLinkerConfig,
    ) -> Result<Self> {
        config.validate()?;

        let decay_engine = DecayEngine::new(storage.clone(), config.decay.clone());

        // Load cursor and cycle count from storage
        let cursor = Self::load_cursor(&storage)?;
        let cycle_count = Self::load_cycle_count(&storage)?;

        let mut metrics = AutoLinkerMetrics::new();
        metrics.update_cursor(cursor);
        metrics.cycles = cycle_count;

        let structural_rules = vec![
            StructuralRule::same_agent(),
            StructuralRule::temporal_proximity(),
            StructuralRule::shared_tags(),
            StructuralRule::decision_to_event(),
            StructuralRule::observation_to_pattern(),
            StructuralRule::fact_supersedes(),
        ];
        let similarity_rule = SimilarityLinkRule;
        let contradiction_detector = ContradictionDetector::new(
            config.similarity.contradiction_threshold,
        );

        Ok(Self {
            storage,
            graph_engine,
            vector_index,
            embedding_service,
            config,
            decay_engine,
            metrics,
            cursor,
            cycle_count,
            structural_rules,
            similarity_rule,
            contradiction_detector,
        })
    }

    /// Load cursor from persistent storage
    fn load_cursor(storage: &Arc<S>) -> Result<DateTime<Utc>> {
        match storage.get_metadata(CURSOR_KEY)? {
            Some(bytes) => {
                let timestamp: i64 = bincode::deserialize(&bytes)
                    .map_err(|e| crate::error::CortexError::Serialization(e))?;
                Ok(DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| Utc::now()))
            }
            None => {
                // Default: 24 hours ago
                Ok(Utc::now() - chrono::Duration::hours(24))
            }
        }
    }

    /// Save cursor to persistent storage
    fn save_cursor(&self) -> Result<()> {
        let timestamp = self.cursor.timestamp();
        let bytes = bincode::serialize(&timestamp)
            .map_err(|e| crate::error::CortexError::Serialization(e))?;
        self.storage.put_metadata(CURSOR_KEY, &bytes)
    }

    /// Load cycle count from persistent storage
    fn load_cycle_count(storage: &Arc<S>) -> Result<u64> {
        match storage.get_metadata(CYCLE_COUNT_KEY)? {
            Some(bytes) => bincode::deserialize(&bytes)
                .map_err(|e| crate::error::CortexError::Serialization(e)),
            None => Ok(0),
        }
    }

    /// Save cycle count to persistent storage
    fn save_cycle_count(&self) -> Result<()> {
        let bytes = bincode::serialize(&self.cycle_count)
            .map_err(|e| crate::error::CortexError::Serialization(e))?;
        self.storage.put_metadata(CYCLE_COUNT_KEY, &bytes)
    }

    /// Run a single processing cycle
    pub fn run_cycle(&mut self) -> Result<()> {
        let start = Instant::now();
        self.metrics.reset_cycle_metrics();

        let now = Utc::now();

        // 1. Scan for new/updated nodes since cursor
        let new_nodes = self.get_nodes_since_cursor()?;

        if new_nodes.is_empty() && self.cycle_count % self.config.decay_every_n_cycles != 0 {
            // Nothing to do this cycle
            self.metrics.set_cycle_duration(start.elapsed());
            return Ok(());
        }

        // 2. Process new nodes (up to max_nodes_per_cycle)
        let nodes_to_process: Vec<_> = new_nodes
            .into_iter()
            .take(self.config.max_nodes_per_cycle)
            .collect();

        let mut proposed_edges = Vec::new();

        for node in &nodes_to_process {
            // Ensure node has embedding
            let embedding = self.ensure_embedding(node)?;

            // Find similar nodes
            let vector_index = self.vector_index.read().unwrap();
            let similar = vector_index.search(&embedding, 100, None)?;
            drop(vector_index);

            let mut node_edge_count = 0;

            // Pre-load existing outgoing edges for this node (batch check)
            let existing_edges = self.storage.edges_from(node.id)?;
            let existing_set: std::collections::HashSet<(NodeId, String)> = existing_edges
                .iter()
                .map(|e| (e.to, format!("{:?}", e.relation)))
                .collect();

            for result in similar {
                // Skip self
                if result.node_id == node.id {
                    continue;
                }

                // Get neighbor node
                let neighbor = match self.storage.get_node(result.node_id)? {
                    Some(n) => n,
                    None => continue,
                };

                // Apply link rules
                let edges = self.apply_link_rules(node, &neighbor, result.score)?;

                // Filter out edges that already exist (using pre-loaded set)
                for edge in edges {
                    if matches!(edge.relation, Relation::Contradicts) {
                        self.metrics.add_contradictions_found(1);
                    }
                    let key = (edge.to, format!("{:?}", edge.relation));
                    if !existing_set.contains(&key) {
                        node_edge_count += 1;
                        proposed_edges.push(edge);
                    }
                }

                // Check per-node limit
                if node_edge_count >= self.config.max_edges_per_node {
                    break;
                }
            }

            // Check for generic content
            if node_edge_count >= self.config.generic_content_threshold {
                log::warn!(
                    "Node {} has {} potential edges, possible generic content",
                    node.id,
                    node_edge_count
                );
            }

            self.metrics.add_nodes_processed(1);

            // Update cursor to this node's timestamp
            if node.created_at > self.cursor {
                self.cursor = node.created_at;
            }
        }

        // 3. Batch-create edges (up to max_edges_per_cycle)
        let edges_to_create: Vec<_> = proposed_edges
            .into_iter()
            .take(self.config.max_edges_per_cycle)
            .collect();

        for proposed in edges_to_create {
            let edge = proposed.to_edge();
            // Edge already pre-filtered in the loop above; just create
            match self.storage.put_edge(&edge) {
                Ok(()) => self.metrics.add_edges_created(1),
                Err(crate::error::CortexError::DuplicateEdge { .. }) => {
                    // Race condition or edge created between check and insert — skip
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        // 4. Decay pass (periodic)
        if self.cycle_count % self.config.decay_every_n_cycles == 0 {
            let (pruned, deleted) = self.decay_engine.apply_decay(now)?;
            self.metrics.add_edges_pruned(pruned);
            self.metrics.add_edges_deleted(deleted);
        }

        // 5. Dedup scan (periodic)
        if self.cycle_count % self.config.dedup_every_n_cycles == 0 {
            let dedup_scanner = DedupScanner::new(
                self.storage.clone(),
                self.vector_index.clone(),
                self.graph_engine.clone(),
                self.config.similarity.clone(),
            );

            let result = dedup_scanner.scan()?;
            self.metrics.add_duplicates_found(result.duplicates.len() as u64);

            // Execute dedup actions
            for pair in result.duplicates {
                dedup_scanner.execute_action(&pair)?;
            }
        }

        // 6. Update metrics and cursor
        self.cycle_count += 1;
        self.metrics.increment_cycle();
        self.metrics.update_cursor(self.cursor);
        self.metrics.set_cycle_duration(start.elapsed());

        // Update stats
        let stats = self.storage.stats()?;
        self.metrics.set_total_nodes(stats.node_count);
        self.metrics.set_total_edges(stats.edge_count);

        // Estimate backlog from what we didn't process this cycle
        self.metrics.set_backlog_size(0); // Will be accurate on next cycle scan

        // Persist cursor and cycle count
        self.save_cursor()?;
        self.save_cycle_count()?;

        log::info!("{}", self.metrics.summary());

        Ok(())
    }

    /// Get nodes created/updated since cursor
    fn get_nodes_since_cursor(&self) -> Result<Vec<Node>> {
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;
        let filtered: Vec<_> = all_nodes
            .into_iter()
            .filter(|n| n.created_at > self.cursor || n.updated_at > self.cursor)
            .filter(|n| !n.deleted)
            .collect();
        Ok(filtered)
    }

    /// Ensure node has an embedding, generate if missing
    fn ensure_embedding(&self, node: &Node) -> Result<Vec<f32>> {
        if let Some(emb) = &node.embedding {
            return Ok(emb.clone());
        }

        // Generate embedding
        let text = embedding_input(node);
        let embedding = self.embedding_service.embed(&text)?;

        // Store in node
        let mut updated = node.clone();
        updated.embedding = Some(embedding.clone());
        self.storage.put_node(&updated)?;

        // Index it
        let mut vector_index = self.vector_index.write().unwrap();
        vector_index.insert(node.id, &embedding)?;
        drop(vector_index);

        Ok(embedding)
    }

    /// Apply all link rules to a node pair
    fn apply_link_rules(
        &self,
        node: &Node,
        neighbor: &Node,
        score: f32,
    ) -> Result<Vec<ProposedEdge>> {
        let mut edges = Vec::new();

        // Similarity rule (pre-allocated)
        if let Some(edge) = self.similarity_rule.evaluate(node, neighbor, score, &self.config.similarity)
        {
            edges.push(edge);
        }

        // Structural rules (pre-allocated)
        for rule in &self.structural_rules {
            if let Some(edge) = rule.evaluate(node, neighbor, score) {
                edges.push(edge);
            }
        }

        // Contradiction detection (pre-allocated)
        if let Some(contradiction) = self.contradiction_detector.check(node, neighbor, score) {
            // Create Contradicts edge
            edges.push(ProposedEdge {
                from: contradiction.node_a,
                to: contradiction.node_b,
                relation: Relation::Contradicts,
                weight: contradiction.similarity,
                provenance: EdgeProvenance::AutoContradiction {
                    reason: contradiction.reason,
                },
            });

        }

        Ok(edges)
    }

    /// Get current metrics
    pub fn metrics(&self) -> &AutoLinkerMetrics {
        &self.metrics
    }

    /// Get current cursor
    pub fn cursor(&self) -> DateTime<Utc> {
        self.cursor
    }

    /// Reinforce edges for a node (called when node is accessed)
    pub fn reinforce(&self, node_id: NodeId) -> Result<u64> {
        self.decay_engine.reinforce(node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphEngineImpl;
    use crate::storage::RedbStorage;
    use crate::types::{NodeKind, Source};
    use crate::vector::{FastEmbedService, HnswIndex, SimilarityConfig};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Requires embedding model
    fn test_auto_linker_cycle() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("auto_linker_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create test nodes
        let node1 = Node::new(
            NodeKind::Fact,
            "Rust programming".into(),
            "Rust is a systems language".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.7,
        );

        let node2 = Node::new(
            NodeKind::Fact,
            "Rust safety".into(),
            "Rust provides memory safety".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.6,
        );

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Setup auto-linker
        let embedding_service = Arc::new(FastEmbedService::new().unwrap());
        let vector_index = Arc::new(RwLock::new(HnswIndex::new(384)));
        let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

        let config = AutoLinkerConfig::new()
            .with_similarity(SimilarityConfig::new().with_auto_link_threshold(0.6));

        let mut linker = AutoLinker::new(
            storage.clone(),
            graph_engine,
            vector_index,
            embedding_service,
            config,
        )
        .unwrap();

        // Run cycle
        linker.run_cycle().unwrap();

        // Check metrics
        let metrics = linker.metrics();
        assert!(metrics.nodes_processed > 0);

        // Should have created some edges
        let stats = storage.stats().unwrap();
        assert!(stats.edge_count > 0);
    }

    #[test]
    fn test_cursor_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("cursor_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        let embedding_service = Arc::new(FastEmbedService::new().unwrap());
        let vector_index = Arc::new(RwLock::new(HnswIndex::new(384)));
        let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

        let config = AutoLinkerConfig::new();

        let linker1 = AutoLinker::new(
            storage.clone(),
            graph_engine.clone(),
            vector_index.clone(),
            embedding_service.clone(),
            config.clone(),
        )
        .unwrap();

        let cursor1 = linker1.cursor();

        // Save cursor
        linker1.save_cursor().unwrap();

        // Create new linker instance
        let linker2 = AutoLinker::new(
            storage.clone(),
            graph_engine,
            vector_index,
            embedding_service,
            config,
        )
        .unwrap();

        let cursor2 = linker2.cursor();

        // Cursors should match (at second precision, since we serialize as i64 seconds)
        assert_eq!(cursor1.timestamp(), cursor2.timestamp());
    }
}
