use crate::error::Result;
use crate::graph::{GraphEngine, TraversalDirection, TraversalRequest};
use crate::storage::Storage;
use crate::types::{Node, NodeId, NodeKind};
use crate::vector::{EmbeddingService, VectorFilter, VectorIndex};
use std::collections::HashMap;
use std::sync::Arc;

/// Query combining vector similarity and graph proximity
#[derive(Debug, Clone)]
pub struct HybridQuery {
    /// Text to search for semantically.
    pub query_text: String,

    /// Optional: bias results toward nodes connected to these anchor nodes.
    /// Graph proximity to anchors boosts ranking.
    pub anchors: Vec<NodeId>,

    /// How much to weight vector similarity vs graph proximity.
    /// 0.0 = pure graph, 1.0 = pure vector. Default 0.7.
    pub vector_weight: f32,

    /// Maximum results.
    pub limit: usize,

    /// Node kind filter.
    pub kind_filter: Option<Vec<NodeKind>>,

    /// Maximum graph distance from anchors to consider.
    /// Nodes beyond this distance get zero graph proximity score.
    pub max_anchor_depth: u32,
}

impl Default for HybridQuery {
    fn default() -> Self {
        Self {
            query_text: String::new(),
            anchors: Vec::new(),
            vector_weight: 0.7,
            limit: 10,
            kind_filter: None,
            max_anchor_depth: 3,
        }
    }
}

impl HybridQuery {
    pub fn new(query_text: String) -> Self {
        Self {
            query_text,
            ..Default::default()
        }
    }

    pub fn with_anchors(mut self, anchors: Vec<NodeId>) -> Self {
        self.anchors = anchors;
        self
    }

    pub fn with_vector_weight(mut self, weight: f32) -> Self {
        self.vector_weight = weight.clamp(0.0, 1.0);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_kind_filter(mut self, kinds: Vec<NodeKind>) -> Self {
        self.kind_filter = Some(kinds);
        self
    }

    pub fn with_max_anchor_depth(mut self, depth: u32) -> Self {
        self.max_anchor_depth = depth;
        self
    }
}

/// Result from hybrid search
#[derive(Debug, Clone)]
pub struct HybridResult {
    pub node: Node,
    pub vector_score: f32,         // Raw cosine similarity
    pub graph_score: f32,          // Proximity to anchors (0.0 - 1.0)
    pub combined_score: f32,       // Weighted blend
    pub nearest_anchor: Option<(NodeId, u32)>, // Closest anchor and depth
}

/// Hybrid search combining vector similarity and graph proximity
pub struct HybridSearch<S: Storage, E: EmbeddingService, V: VectorIndex, G: GraphEngine> {
    storage: Arc<S>,
    embedding_service: E,
    vector_index: V,
    graph_engine: G,
}

impl<S: Storage, E: EmbeddingService, V: VectorIndex, G: GraphEngine> HybridSearch<S, E, V, G> {
    pub fn new(storage: Arc<S>, embedding_service: E, vector_index: V, graph_engine: G) -> Self {
        Self {
            storage,
            embedding_service,
            vector_index,
            graph_engine,
        }
    }

    /// Execute a hybrid query
    pub fn search(&self, query: HybridQuery) -> Result<Vec<HybridResult>> {
        // 1. Generate embedding for query text
        let query_embedding = self.embedding_service.embed(&query.query_text)?;

        // 2. Vector search
        let vector_filter = query
            .kind_filter
            .as_ref()
            .map(|kinds| VectorFilter::new().with_kinds(kinds.clone()));

        let vector_results = self.vector_index.search(
            &query_embedding,
            query.limit * 3, // Get more candidates for graph filtering
            vector_filter.as_ref(),
        )?;

        // 3. If no anchors, return pure vector results
        if query.anchors.is_empty() {
            let mut results = Vec::new();
            for vr in vector_results.into_iter().take(query.limit) {
                if let Some(node) = self.storage.get_node(vr.node_id)? {
                    results.push(HybridResult {
                        node,
                        vector_score: vr.score,
                        graph_score: 0.0,
                        combined_score: vr.score,
                        nearest_anchor: None,
                    });
                }
            }
            return Ok(results);
        }

        // 4. Compute graph proximity scores
        let graph_scores = self.compute_graph_proximity(&query.anchors, query.max_anchor_depth)?;

        // 5. Combine scores and rank
        let mut hybrid_results = Vec::new();

        for vr in vector_results {
            if let Some(node) = self.storage.get_node(vr.node_id)? {
                let graph_score = graph_scores
                    .get(&vr.node_id)
                    .map(|(score, _, _)| *score)
                    .unwrap_or(0.0);

                let nearest_anchor = graph_scores
                    .get(&vr.node_id)
                    .and_then(|(_, anchor, depth)| anchor.map(|a| (a, *depth)));

                let combined_score = (query.vector_weight * vr.score)
                    + ((1.0 - query.vector_weight) * graph_score);

                hybrid_results.push(HybridResult {
                    node,
                    vector_score: vr.score,
                    graph_score,
                    combined_score,
                    nearest_anchor,
                });
            }
        }

        // Sort by combined score descending
        hybrid_results.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Take top results
        Ok(hybrid_results.into_iter().take(query.limit).collect())
    }

    /// Compute graph proximity scores for all nodes relative to anchors
    /// Returns: NodeId -> (score, nearest_anchor_id, depth_to_anchor)
    fn compute_graph_proximity(
        &self,
        anchors: &[NodeId],
        max_depth: u32,
    ) -> Result<HashMap<NodeId, (f32, Option<NodeId>, u32)>> {
        let mut proximity_scores = HashMap::new();

        for anchor_id in anchors {
            // Traverse from each anchor
            let neighborhood = self.graph_engine.traverse(TraversalRequest {
                start: vec![*anchor_id],
                max_depth: Some(max_depth),
                direction: TraversalDirection::Both,
                include_start: false,
                ..Default::default()
            })?;

            // Score based on depth: score = 1.0 / (1.0 + depth)
            for (node_id, &depth) in &neighborhood.depths {
                let score = 1.0 / (1.0 + depth as f32);

                // Keep the highest score (shortest path) to any anchor
                proximity_scores
                    .entry(*node_id)
                    .and_modify(|(existing_score, existing_anchor, existing_depth)| {
                        if score > *existing_score {
                            *existing_score = score;
                            *existing_anchor = Some(*anchor_id);
                            *existing_depth = depth;
                        }
                    })
                    .or_insert((score, Some(*anchor_id), depth));
            }
        }

        Ok(proximity_scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphEngineImpl;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, Relation, Source};
    use crate::vector::{FastEmbedService, HnswIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Requires model download
    fn test_hybrid_search_no_anchors() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("hybrid_test.redb");

        let storage = RedbStorage::open(&db_path).unwrap();

        // Create test nodes
        let node1 = Node::new(
            NodeKind::Fact,
            "Rust programming language".to_string(),
            "Rust is a systems programming language".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );

        let node2 = Node::new(
            NodeKind::Fact,
            "Python programming language".to_string(),
            "Python is a high-level programming language".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Create embeddings and index
        let embedding_service = FastEmbedService::new().unwrap();
        let mut vector_index = HnswIndex::new(384);

        let emb1 = embedding_service
            .embed(&crate::vector::embedding_input(&node1))
            .unwrap();
        let emb2 = embedding_service
            .embed(&crate::vector::embedding_input(&node2))
            .unwrap();

        vector_index.insert(node1.id, &emb1).unwrap();
        vector_index.insert(node2.id, &emb2).unwrap();
        vector_index.rebuild().unwrap();

        let storage_arc = Arc::new(storage);
        let graph_engine = GraphEngineImpl::new(storage_arc.clone());

        let hybrid = HybridSearch::new(storage_arc.clone(), embedding_service, vector_index, graph_engine);

        let query = HybridQuery::new("Rust language".to_string()).with_limit(5);

        let results = hybrid.search(query).unwrap();

        assert!(!results.is_empty());
        // First result should be about Rust
        assert!(results[0].node.data.title.contains("Rust"));
    }

    #[test]
    #[ignore] // Requires model download
    fn test_hybrid_search_with_anchors() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("hybrid_anchor_test.redb");

        let storage = RedbStorage::open(&db_path).unwrap();

        // Create connected nodes
        let anchor_node = Node::new(
            NodeKind::Decision,
            "Use Rust for backend".to_string(),
            "Decision to use Rust".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.8,
        );

        let connected_node = Node::new(
            NodeKind::Fact,
            "Rust is fast and safe".to_string(),
            "Rust provides memory safety without garbage collection".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.7,
        );

        let unconnected_node = Node::new(
            NodeKind::Fact,
            "Rust has great tooling".to_string(),
            "Cargo is Rust's package manager".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.6,
        );

        storage.put_node(&anchor_node).unwrap();
        storage.put_node(&connected_node).unwrap();
        storage.put_node(&unconnected_node).unwrap();

        // Create edge connecting anchor to one node
        let edge = Edge::new(
            anchor_node.id,
            connected_node.id,
            Relation::InformedBy,
            0.9,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        // Setup embeddings
        let embedding_service = FastEmbedService::new().unwrap();
        let mut vector_index = HnswIndex::new(384);

        for node in [&anchor_node, &connected_node, &unconnected_node] {
            let emb = embedding_service
                .embed(&crate::vector::embedding_input(node))
                .unwrap();
            vector_index.insert(node.id, &emb).unwrap();
        }
        vector_index.rebuild().unwrap();

        let storage_arc = Arc::new(storage);
        let graph_engine = GraphEngineImpl::new(storage_arc.clone());

        let hybrid = HybridSearch::new(storage_arc.clone(), embedding_service, vector_index, graph_engine);

        let query = HybridQuery::new("Rust properties".to_string())
            .with_anchors(vec![anchor_node.id])
            .with_vector_weight(0.5); // Balance vector and graph

        let results = hybrid.search(query).unwrap();

        // Connected node should rank higher due to graph proximity
        assert!(!results.is_empty());
        let connected_result = results
            .iter()
            .find(|r| r.node.id == connected_node.id)
            .unwrap();
        assert!(connected_result.graph_score > 0.0);
    }
}
