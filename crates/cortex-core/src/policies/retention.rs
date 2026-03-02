use crate::error::{CortexError, Result};
use crate::storage::{NodeFilter, Storage};
use crate::types::{Node, NodeId, NodeKind};
use crate::vector::{apply_score_decay, ScoreDecayConfig};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Per-kind retention settings.
///
/// Supports deserializing from either a bare integer (backward-compat):
///   `observation = 90`
/// or a full table:
///   `observation = { ttl_days = 90, min_score = 0.15 }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindRetention {
    pub ttl_days: u64,
    /// Minimum decayed score to keep the node alive past TTL.
    /// `None` = no score check (pure age-based, backward-compat).
    #[serde(default)]
    pub min_score: Option<f32>,
}

/// Allow bare integers in TOML by implementing a custom deserializer.
fn deserialize_by_kind<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, KindRetention>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{MapAccess, Visitor};

    /// Accepts either an integer or a { ttl_days, min_score } table.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum KindRetentionOrU64 {
        Full(KindRetention),
        Days(u64),
    }

    struct ByKindVisitor;
    impl<'de> Visitor<'de> for ByKindVisitor {
        type Value = HashMap<String, KindRetention>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map of kind → ttl_days (u64) or { ttl_days, min_score }")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = HashMap::new();
            while let Some((key, value)) = map.next_entry::<String, KindRetentionOrU64>()? {
                let kr = match value {
                    KindRetentionOrU64::Full(kr) => kr,
                    KindRetentionOrU64::Days(d) => KindRetention {
                        ttl_days: d,
                        min_score: None,
                    },
                };
                result.insert(key, kr);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(ByKindVisitor)
}

/// Retention configuration (mirrors cortex-server's CortexConfig retention block).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetentionConfig {
    /// Default TTL for all nodes in days. 0 = keep forever.
    pub default_ttl_days: u64,
    /// Per-kind TTLs and optional score gates.
    /// Supports bare integers (`observation = 90`) for backward compatibility,
    /// or full tables (`observation = { ttl_days = 90, min_score = 0.15 }`).
    #[serde(default, deserialize_with = "deserialize_by_kind")]
    pub by_kind: HashMap<String, KindRetention>,
    /// Hard cap on total live node count.
    pub max_nodes: Option<RetentionMaxNodes>,
    /// Days of inactivity (since last access) required beyond TTL before deletion.
    /// Default: 30. A node accessed within this window survives even past TTL.
    #[serde(default = "default_grace_days")]
    pub grace_days: u64,
    /// Don't soft-delete a node if live (non-deleted) nodes still reference it
    /// via inbound edges. Default: true.
    #[serde(default = "default_true")]
    pub protect_with_inbound_edges: bool,
}

fn default_grace_days() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

/// Strategy configuration for max-node eviction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionMaxNodes {
    pub limit: usize,
    pub strategy: String,
}

/// Drives node expiry based on TTL, score decay, access recency, and edge protection.
pub struct RetentionEngine {
    config: RetentionConfig,
    score_decay_config: ScoreDecayConfig,
}

impl RetentionEngine {
    pub fn new(config: RetentionConfig, score_decay_config: ScoreDecayConfig) -> Self {
        Self {
            config,
            score_decay_config,
        }
    }

    /// Check whether a single node is eligible for conditional deletion.
    /// All conditions must be true for the node to be deletable.
    fn should_delete<S: Storage>(
        &self,
        node: &Node,
        kind_retention: &KindRetention,
        storage: &S,
    ) -> Result<bool> {
        let now = Utc::now();

        // 1. Age exceeds TTL
        let age_days = (now - node.created_at).num_days();
        if age_days <= kind_retention.ttl_days as i64 {
            return Ok(false);
        }

        // 2. Score below minimum (if configured)
        if let Some(min_score) = kind_retention.min_score {
            // Use apply_score_decay with raw_score=1.0 and full recency bias
            // to get the pure temporal+access relevance factor
            let decayed = apply_score_decay(node, 1.0, &self.score_decay_config, 1.0);
            if decayed >= min_score {
                return Ok(false);
            }
        }

        // 3. Not accessed recently (within grace period)
        let days_since_access = (now - node.last_accessed_at).num_days();
        if days_since_access <= self.config.grace_days as i64 {
            return Ok(false);
        }

        // 4. No live inbound edges (if protection enabled)
        if self.config.protect_with_inbound_edges {
            let inbound = storage.edges_to(node.id)?;
            for edge in &inbound {
                // Check if the source node is still alive
                if let Some(source) = storage.get_node(edge.from)? {
                    if !source.deleted {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    /// Soft-delete outbound edges from a node being deleted.
    /// Prevents stale edges from influencing the linker between sweep and purge.
    fn cleanup_outbound_edges<S: Storage>(&self, node_id: NodeId, storage: &S) -> Result<()> {
        let outbound = storage.edges_from(node_id)?;
        for edge in outbound {
            storage.delete_edge(edge.id)?;
        }
        Ok(())
    }

    /// Soft-delete nodes that have exceeded their TTL and meet all retention conditions,
    /// or breach the max-nodes cap.
    /// Returns the number of nodes soft-deleted this sweep.
    pub fn sweep<S: Storage>(&self, storage: &S) -> Result<usize> {
        let mut deleted = 0;
        let now = Utc::now();

        // 1. Per-kind TTLs with conditional checks
        for (kind_str, kind_retention) in &self.config.by_kind {
            if kind_retention.ttl_days == 0 {
                continue;
            }
            let kind = match NodeKind::new(kind_str) {
                Ok(k) => k,
                Err(_) => continue, // skip invalid kind strings in config
            };
            let cutoff = now - Duration::days(kind_retention.ttl_days as i64);
            let candidates = storage.list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![kind])
                    .created_before(cutoff),
            )?;
            for node in candidates {
                if self.should_delete(&node, kind_retention, storage)? {
                    self.cleanup_outbound_edges(node.id, storage)?;
                    storage.delete_node(node.id)?;
                    deleted += 1;
                }
            }
        }

        // 2. Default TTL across all kinds not pinned to 0
        if self.config.default_ttl_days > 0 {
            let cutoff = now - Duration::days(self.config.default_ttl_days as i64);
            let expired = storage.list_nodes(NodeFilter::new().created_before(cutoff))?;
            let default_retention = KindRetention {
                ttl_days: self.config.default_ttl_days,
                min_score: None,
            };
            for node in expired {
                let kind_str = node.kind.as_str().to_string();
                // Skip kinds with explicit config (already handled above, or kept forever at 0)
                if self.config.by_kind.contains_key(&kind_str) {
                    continue;
                }
                if self.should_delete(&node, &default_retention, storage)? {
                    self.cleanup_outbound_edges(node.id, storage)?;
                    storage.delete_node(node.id)?;
                    deleted += 1;
                }
            }
        }

        // 3. Max node cap (unchanged — eviction is immediate, not conditional)
        if let Some(max_cfg) = &self.config.max_nodes {
            let stats = storage.stats()?;
            if stats.node_count as usize > max_cfg.limit {
                let excess = stats.node_count as usize - max_cfg.limit;
                let to_evict =
                    self.select_eviction_candidates(storage, excess, &max_cfg.strategy)?;
                for id in to_evict {
                    self.cleanup_outbound_edges(id, storage)?;
                    storage.delete_node(id)?;
                    deleted += 1;
                }
            }
        }

        Ok(deleted)
    }

    /// Hard-delete nodes that have been soft-deleted beyond the grace period.
    /// Returns the number of nodes hard-deleted.
    pub fn purge_expired<S: Storage>(&self, storage: &S) -> Result<usize> {
        let grace = if self.config.grace_days == 0 {
            30
        } else {
            self.config.grace_days
        };
        let cutoff = Utc::now() - Duration::days(grace as i64);

        // Only fetch soft-deleted nodes updated before the grace cutoff
        let candidates =
            storage.list_nodes(NodeFilter::new().deleted_only().updated_before(cutoff))?;
        let mut purged = 0;
        for node in candidates {
            storage.hard_delete_node(node.id)?;
            purged += 1;
        }
        Ok(purged)
    }

    fn select_eviction_candidates<S: Storage>(
        &self,
        storage: &S,
        count: usize,
        strategy: &str,
    ) -> Result<Vec<Uuid>> {
        match strategy {
            "oldest_lowest_importance" => {
                let mut nodes = storage.list_nodes(NodeFilter::new())?;
                nodes.sort_by(|a, b| {
                    a.importance
                        .partial_cmp(&b.importance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.created_at.cmp(&b.created_at))
                });
                Ok(nodes.into_iter().take(count).map(|n| n.id).collect())
            }
            _ => Err(CortexError::Validation(format!(
                "Unknown eviction strategy: {}",
                strategy
            ))),
        }
    }
}

/// Represents a node that has been soft-deleted and is eligible for hard deletion.
#[derive(Debug)]
pub struct PendingPurge {
    pub id: NodeId,
    pub deleted_at: chrono::DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, Node, NodeKind, Relation, Source};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_storage() -> (Arc<RedbStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.redb");
        (Arc::new(RedbStorage::open(&path).unwrap()), dir)
    }

    fn make_node(kind: &str, importance: f32) -> Node {
        Node::new(
            NodeKind::new(kind).unwrap(),
            format!("Test {kind}"),
            "Body".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            importance,
        )
    }

    fn default_score_decay() -> ScoreDecayConfig {
        ScoreDecayConfig::default()
    }

    #[test]
    fn test_sweep_no_config_is_noop() {
        let (storage, _dir) = make_storage();
        let node = make_node("fact", 0.5);
        storage.put_node(&node).unwrap();

        let engine = RetentionEngine::new(RetentionConfig::default(), default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 0);

        let retrieved = storage.get_node(node.id).unwrap();
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_sweep_default_ttl_expires_old_nodes() {
        let (storage, _dir) = make_storage();

        // Create a node with an old created_at and old last_accessed_at (past grace)
        let mut old_node = make_node("fact", 0.5);
        old_node.created_at = Utc::now() - Duration::days(100);
        old_node.last_accessed_at = Utc::now() - Duration::days(90);
        storage.put_node(&old_node).unwrap();

        let mut new_node = make_node("fact", 0.5);
        new_node.created_at = Utc::now();
        storage.put_node(&new_node).unwrap();

        let config = RetentionConfig {
            default_ttl_days: 7,
            grace_days: 30,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1);

        // Old node should be soft-deleted
        let retrieved = storage.get_node(old_node.id).unwrap().unwrap();
        assert!(retrieved.deleted);

        // New node should still be alive
        let retrieved = storage.get_node(new_node.id).unwrap().unwrap();
        assert!(!retrieved.deleted);
    }

    #[test]
    fn test_sweep_by_kind_ttl() {
        let (storage, _dir) = make_storage();

        // Old observation — should be expired by by_kind TTL
        let mut obs = make_node("observation", 0.5);
        obs.created_at = Utc::now() - Duration::days(100);
        obs.last_accessed_at = Utc::now() - Duration::days(90);
        storage.put_node(&obs).unwrap();

        // Old decision — kind has ttl=0, should be kept forever
        let mut dec = make_node("decision", 0.5);
        dec.created_at = Utc::now() - Duration::days(100);
        dec.last_accessed_at = Utc::now() - Duration::days(90);
        storage.put_node(&dec).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: None,
            },
        );
        by_kind.insert(
            "decision".to_string(),
            KindRetention {
                ttl_days: 0,
                min_score: None,
            },
        );

        let config = RetentionConfig {
            by_kind,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1);

        assert!(storage.get_node(obs.id).unwrap().unwrap().deleted);
        assert!(!storage.get_node(dec.id).unwrap().unwrap().deleted);
    }

    #[test]
    fn test_sweep_max_nodes_evicts_least_important() {
        let (storage, _dir) = make_storage();

        let mut low = make_node("fact", 0.1);
        low.created_at = Utc::now() - Duration::days(5);
        let mut high = make_node("fact", 0.9);
        high.created_at = Utc::now() - Duration::days(3);

        storage.put_node(&low).unwrap();
        storage.put_node(&high).unwrap();

        let config = RetentionConfig {
            max_nodes: Some(RetentionMaxNodes {
                limit: 1,
                strategy: "oldest_lowest_importance".to_string(),
            }),
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1);

        // Low importance node should be evicted
        assert!(storage.get_node(low.id).unwrap().unwrap().deleted);
        assert!(!storage.get_node(high.id).unwrap().unwrap().deleted);
    }

    #[test]
    fn test_purge_expired_hard_deletes_old_soft_deletes() {
        let (storage, _dir) = make_storage();

        let node = make_node("fact", 0.5);
        storage.put_node(&node).unwrap();
        storage.delete_node(node.id).unwrap();

        // Manually set updated_at to past the grace period
        let mut deleted_node = storage
            .list_nodes(NodeFilter::new().include_deleted())
            .unwrap()
            .into_iter()
            .find(|n| n.id == node.id)
            .unwrap();
        deleted_node.updated_at = Utc::now() - Duration::days(60);
        storage.put_node(&deleted_node).unwrap();

        let config = RetentionConfig {
            grace_days: 30,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let purged = engine.purge_expired(storage.as_ref()).unwrap();
        assert_eq!(purged, 1);

        // Node should be completely gone
        assert!(storage.get_node(node.id).unwrap().is_none());
    }

    // ── New conditional retention tests ──

    #[test]
    fn test_recently_accessed_node_survives_past_ttl() {
        let (storage, _dir) = make_storage();

        // Old node that was accessed recently (within grace period)
        let mut node = make_node("observation", 0.5);
        node.created_at = Utc::now() - Duration::days(100);
        node.last_accessed_at = Utc::now() - Duration::days(5); // accessed 5 days ago
        storage.put_node(&node).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: None,
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 0, "Recently accessed node should survive past TTL");

        assert!(!storage.get_node(node.id).unwrap().unwrap().deleted);
    }

    #[test]
    fn test_node_with_live_inbound_edge_survives_past_ttl() {
        let (storage, _dir) = make_storage();

        // Old target node past TTL and grace
        let mut target = make_node("observation", 0.3);
        target.created_at = Utc::now() - Duration::days(200);
        target.last_accessed_at = Utc::now() - Duration::days(100);
        storage.put_node(&target).unwrap();

        // Live source node referencing the target
        let source = make_node("fact", 0.8);
        storage.put_node(&source).unwrap();

        // Create edge from source → target
        let edge = Edge::new(
            source.id,
            target.id,
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::AutoSimilarity { score: 0.8 },
        );
        storage.put_edge(&edge).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: None,
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: true,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 0, "Node with live inbound edge should survive");

        assert!(!storage.get_node(target.id).unwrap().unwrap().deleted);
    }

    #[test]
    fn test_node_meets_all_conditions_is_deleted() {
        let (storage, _dir) = make_storage();

        // Node that is old, stale, not accessed, no inbound edges
        let mut node = make_node("observation", 0.1);
        node.created_at = Utc::now() - Duration::days(200);
        node.last_accessed_at = Utc::now() - Duration::days(150);
        node.access_count = 0;
        storage.put_node(&node).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: Some(0.5),
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: true,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1, "Node meeting all conditions should be deleted");

        assert!(storage.get_node(node.id).unwrap().unwrap().deleted);
    }

    #[test]
    fn test_outbound_edges_removed_on_soft_delete() {
        let (storage, _dir) = make_storage();

        // Node that will be deleted
        let mut doomed = make_node("observation", 0.1);
        doomed.created_at = Utc::now() - Duration::days(200);
        doomed.last_accessed_at = Utc::now() - Duration::days(150);
        storage.put_node(&doomed).unwrap();

        // Another node that the doomed node links to
        let target = make_node("fact", 0.9);
        storage.put_node(&target).unwrap();

        // Create outbound edge from doomed → target
        let edge = Edge::new(
            doomed.id,
            target.id,
            Relation::new("related_to").unwrap(),
            0.7,
            EdgeProvenance::AutoSimilarity { score: 0.7 },
        );
        storage.put_edge(&edge).unwrap();

        // Verify edge exists
        assert_eq!(storage.edges_from(doomed.id).unwrap().len(), 1);

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: None,
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1);

        // Outbound edges should be cleaned up
        assert_eq!(
            storage.edges_from(doomed.id).unwrap().len(),
            0,
            "Outbound edges should be removed on soft-delete"
        );
    }

    #[test]
    fn test_min_score_none_skips_score_check() {
        let (storage, _dir) = make_storage();

        // Node past TTL and grace but with no min_score configured
        let mut node = make_node("event", 0.9); // high importance
        node.created_at = Utc::now() - Duration::days(200);
        node.last_accessed_at = Utc::now() - Duration::days(150);
        node.access_count = 100; // heavily used
        storage.put_node(&node).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "event".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: None, // no score gate
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 1, "Without min_score, age + grace is sufficient");
    }

    #[test]
    fn test_high_score_survives_with_min_score_gate() {
        let (storage, _dir) = make_storage();

        // Node past TTL and grace but with high access count → high decayed score
        let mut node = make_node("observation", 0.9);
        node.created_at = Utc::now() - Duration::days(200);
        // Access it recently-ish so temporal factor isn't too low,
        // but past grace period
        node.last_accessed_at = Utc::now() - Duration::days(35);
        node.access_count = 50; // echo boost
        storage.put_node(&node).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert(
            "observation".to_string(),
            KindRetention {
                ttl_days: 30,
                min_score: Some(0.05), // very low bar, but echo boost should keep it above
            },
        );

        let config = RetentionConfig {
            by_kind,
            grace_days: 30,
            protect_with_inbound_edges: false,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config, default_score_decay());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(
            deleted, 0,
            "High-score node should survive with min_score gate"
        );
    }
}
