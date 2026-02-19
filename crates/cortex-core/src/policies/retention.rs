use crate::error::{CortexError, Result};
use crate::storage::{NodeFilter, Storage};
use crate::types::{NodeId, NodeKind};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Retention configuration (mirrors cortex-server's CortexConfig retention block).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetentionConfig {
    /// Default TTL for all nodes in days. 0 = keep forever.
    pub default_ttl_days: u64,
    /// Per-kind TTLs (kind name → days). 0 = keep forever for that kind.
    #[serde(default)]
    pub by_kind: HashMap<String, u64>,
    /// Hard cap on total live node count.
    pub max_nodes: Option<RetentionMaxNodes>,
    /// Days to keep soft-deleted nodes before hard-deletion. Default: 7.
    #[serde(default = "default_grace_days")]
    pub grace_days: u64,
}

fn default_grace_days() -> u64 {
    7
}

/// Strategy configuration for max-node eviction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionMaxNodes {
    pub limit: usize,
    pub strategy: String,
}

/// Drives node expiry based on TTL and count caps.
pub struct RetentionEngine {
    config: RetentionConfig,
}

impl RetentionEngine {
    pub fn new(config: RetentionConfig) -> Self {
        Self { config }
    }

    /// Soft-delete nodes that have exceeded their TTL or breach the max-nodes cap.
    /// Returns the number of nodes soft-deleted this sweep.
    pub fn sweep<S: Storage>(&self, storage: &S) -> Result<usize> {
        let mut deleted = 0;
        let now = Utc::now();

        // 1. Per-kind TTLs
        for (kind_str, &ttl_days) in &self.config.by_kind {
            if ttl_days == 0 {
                continue;
            }
            let kind = match NodeKind::new(kind_str) {
                Ok(k) => k,
                Err(_) => continue, // skip invalid kind strings in config
            };
            let cutoff = now - Duration::days(ttl_days as i64);
            let expired = storage.list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![kind])
                    .created_before(cutoff),
            )?;
            for node in expired {
                storage.delete_node(node.id)?;
                deleted += 1;
            }
        }

        // 2. Default TTL across all kinds not pinned to 0
        if self.config.default_ttl_days > 0 {
            let cutoff = now - Duration::days(self.config.default_ttl_days as i64);
            let expired = storage.list_nodes(NodeFilter::new().created_before(cutoff))?;
            for node in expired {
                let kind_str = node.kind.as_str().to_string();
                // Skip kinds explicitly set to 0 (keep forever)
                if self.config.by_kind.get(&kind_str).copied() == Some(0) {
                    continue;
                }
                storage.delete_node(node.id)?;
                deleted += 1;
            }
        }

        // 3. Max node cap
        if let Some(max_cfg) = &self.config.max_nodes {
            let stats = storage.stats()?;
            if stats.node_count as usize > max_cfg.limit {
                let excess = stats.node_count as usize - max_cfg.limit;
                let to_evict =
                    self.select_eviction_candidates(storage, excess, &max_cfg.strategy)?;
                for id in to_evict {
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
            7
        } else {
            self.config.grace_days
        };
        let cutoff = Utc::now() - Duration::days(grace as i64);

        let all_nodes = storage.list_nodes(NodeFilter::new().include_deleted())?;
        let mut purged = 0;
        for node in all_nodes {
            if node.deleted && node.updated_at < cutoff {
                storage.hard_delete_node(node.id)?;
                purged += 1;
            }
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
    use crate::types::{Node, NodeKind, Source};
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

    #[test]
    fn test_sweep_no_config_is_noop() {
        let (storage, _dir) = make_storage();
        let node = make_node("fact", 0.5);
        storage.put_node(&node).unwrap();

        let engine = RetentionEngine::new(RetentionConfig::default());
        let deleted = engine.sweep(storage.as_ref()).unwrap();
        assert_eq!(deleted, 0);

        let retrieved = storage.get_node(node.id).unwrap();
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_sweep_default_ttl_expires_old_nodes() {
        let (storage, _dir) = make_storage();

        // Create a node with an old created_at
        let mut old_node = make_node("fact", 0.5);
        old_node.created_at = Utc::now() - Duration::days(10);
        storage.put_node(&old_node).unwrap();

        let mut new_node = make_node("fact", 0.5);
        new_node.created_at = Utc::now();
        storage.put_node(&new_node).unwrap();

        let config = RetentionConfig {
            default_ttl_days: 7,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config);
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
        obs.created_at = Utc::now() - Duration::days(40);
        storage.put_node(&obs).unwrap();

        // Old decision — kind has ttl=0, should be kept forever
        let mut dec = make_node("decision", 0.5);
        dec.created_at = Utc::now() - Duration::days(40);
        storage.put_node(&dec).unwrap();

        let mut by_kind = HashMap::new();
        by_kind.insert("observation".to_string(), 30u64);
        by_kind.insert("decision".to_string(), 0u64);

        let config = RetentionConfig {
            by_kind,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config);
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
        let engine = RetentionEngine::new(config);
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
        deleted_node.updated_at = Utc::now() - Duration::days(10);
        storage.put_node(&deleted_node).unwrap();

        let config = RetentionConfig {
            grace_days: 7,
            ..Default::default()
        };
        let engine = RetentionEngine::new(config);
        let purged = engine.purge_expired(storage.as_ref()).unwrap();
        assert_eq!(purged, 1);

        // Node should be completely gone
        assert!(storage.get_node(node.id).unwrap().is_none());
    }
}
