use chrono::{DateTime, Utc};
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const AUDIT_TABLE: TableDefinition<u128, &[u8]> = TableDefinition::new("audit");

/// A single record of a mutation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When this action occurred.
    pub timestamp: DateTime<Utc>,
    /// What happened.
    pub action: AuditAction,
    /// The node or edge that was affected.
    pub target_id: Uuid,
    /// Which agent or process caused this action.
    pub actor: String,
    /// Optional diff or description.
    pub details: Option<String>,
}

/// The type of mutation that was recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditAction {
    NodeCreated,
    NodeUpdated,
    NodeDeleted,
    NodeHardDeleted,
    EdgeCreated,
    EdgeDecayed,
    EdgePruned,
    NodeMerged,
    BriefingGenerated,
    SchemaUpgraded,
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditAction::NodeCreated => write!(f, "node.created"),
            AuditAction::NodeUpdated => write!(f, "node.updated"),
            AuditAction::NodeDeleted => write!(f, "node.deleted"),
            AuditAction::NodeHardDeleted => write!(f, "node.hard_deleted"),
            AuditAction::EdgeCreated => write!(f, "edge.created"),
            AuditAction::EdgeDecayed => write!(f, "edge.decayed"),
            AuditAction::EdgePruned => write!(f, "edge.pruned"),
            AuditAction::NodeMerged => write!(f, "node.merged"),
            AuditAction::BriefingGenerated => write!(f, "briefing.generated"),
            AuditAction::SchemaUpgraded => write!(f, "schema.upgraded"),
        }
    }
}

/// Append-only log of every mutation, stored in a dedicated redb table.
pub struct AuditLog {
    db: Arc<Database>,
    /// Monotonic counter to disambiguate entries within the same nanosecond.
    seq: AtomicU64,
}

impl AuditLog {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            seq: AtomicU64::new(0),
        }
    }

    /// Append an audit entry. Key is timestamp_nanos for time-ordered iteration.
    pub fn log(&self, entry: AuditEntry) -> crate::Result<()> {
        let nanos = entry.timestamp.timestamp_nanos_opt().unwrap_or(0) as u128;
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) as u128;
        let key = (nanos << 32) | (seq & 0xFFFF_FFFF);
        let value = serde_json::to_vec(&entry)
            .map_err(|e| crate::CortexError::Validation(format!("Audit serialise: {}", e)))?;

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| crate::CortexError::Validation(format!("Audit write: {}", e)))?;
        {
            let mut table = write_txn
                .open_table(AUDIT_TABLE)
                .map_err(|e| crate::CortexError::Validation(format!("Audit table: {}", e)))?;
            table
                .insert(key, value.as_slice())
                .map_err(|e| crate::CortexError::Validation(format!("Audit insert: {}", e)))?;
        }
        write_txn
            .commit()
            .map_err(|e| crate::CortexError::Validation(format!("Audit commit: {}", e)))?;
        Ok(())
    }

    /// Query audit entries with optional filters.
    pub fn query(&self, filter: AuditFilter) -> crate::Result<Vec<AuditEntry>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| crate::CortexError::Validation(format!("Audit read: {}", e)))?;
        let table = read_txn
            .open_table(AUDIT_TABLE)
            .map_err(|e| crate::CortexError::Validation(format!("Audit table: {}", e)))?;

        let since_nanos = filter
            .since
            .and_then(|t| t.timestamp_nanos_opt())
            .map(|n| n as u128)
            .unwrap_or(0);

        let mut entries = Vec::new();
        for result in table
            .range(since_nanos..)
            .map_err(|e| crate::CortexError::Validation(format!("Audit range: {}", e)))?
        {
            let (_, value) =
                result.map_err(|e| crate::CortexError::Validation(format!("Audit iter: {}", e)))?;
            let entry = match serde_json::from_slice::<AuditEntry>(value.value()) {
                Ok(e) => e,
                Err(_) => continue, // skip corrupt entries
            };

            if let Some(ref actor) = filter.actor {
                if entry.actor != *actor {
                    continue;
                }
            }
            if let Some(ref node_id) = filter.node_id {
                if entry.target_id != *node_id {
                    continue;
                }
            }
            if let Some(ref action) = filter.action {
                if entry.action != *action {
                    continue;
                }
            }

            entries.push(entry);
            if let Some(limit) = filter.limit {
                if entries.len() >= limit {
                    break;
                }
            }
        }

        Ok(entries)
    }
}

/// Filter criteria for querying the audit log.
#[derive(Debug, Default)]
pub struct AuditFilter {
    /// Only entries at or after this timestamp.
    pub since: Option<DateTime<Utc>>,
    /// Only entries by this actor.
    pub actor: Option<String>,
    /// Only entries for this node/edge ID.
    pub node_id: Option<Uuid>,
    /// Only entries of this action type.
    pub action: Option<AuditAction>,
    /// Maximum number of entries to return.
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::Database;
    use tempfile::TempDir;

    fn make_audit_log() -> (AuditLog, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("audit_test.redb");
        let db = Arc::new(Database::create(&db_path).unwrap());
        // Initialize the audit table
        let txn = db.begin_write().unwrap();
        txn.open_table(AUDIT_TABLE).unwrap();
        txn.commit().unwrap();
        (AuditLog::new(db), dir)
    }

    fn make_entry(action: AuditAction, actor: &str) -> AuditEntry {
        AuditEntry {
            timestamp: Utc::now(),
            action,
            target_id: Uuid::now_v7(),
            actor: actor.to_string(),
            details: None,
        }
    }

    #[test]
    fn test_log_and_query_all() {
        let (log, _dir) = make_audit_log();
        log.log(make_entry(AuditAction::NodeCreated, "kai"))
            .unwrap();
        log.log(make_entry(AuditAction::EdgeCreated, "auto-linker"))
            .unwrap();

        let entries = log.query(AuditFilter::default()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_query_filter_by_actor() {
        let (log, _dir) = make_audit_log();
        log.log(make_entry(AuditAction::NodeCreated, "kai"))
            .unwrap();
        log.log(make_entry(AuditAction::EdgeCreated, "auto-linker"))
            .unwrap();

        let entries = log
            .query(AuditFilter {
                actor: Some("kai".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor, "kai");
    }

    #[test]
    fn test_query_filter_by_action() {
        let (log, _dir) = make_audit_log();
        log.log(make_entry(AuditAction::NodeCreated, "kai"))
            .unwrap();
        log.log(make_entry(AuditAction::NodeUpdated, "kai"))
            .unwrap();
        log.log(make_entry(AuditAction::EdgeCreated, "auto-linker"))
            .unwrap();

        let entries = log
            .query(AuditFilter {
                action: Some(AuditAction::NodeCreated),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, AuditAction::NodeCreated);
    }

    #[test]
    fn test_query_filter_by_node_id() {
        let (log, _dir) = make_audit_log();
        let target = Uuid::now_v7();
        log.log(AuditEntry {
            timestamp: Utc::now(),
            action: AuditAction::NodeCreated,
            target_id: target,
            actor: "kai".into(),
            details: None,
        })
        .unwrap();
        log.log(make_entry(AuditAction::NodeCreated, "kai"))
            .unwrap();

        let entries = log
            .query(AuditFilter {
                node_id: Some(target),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].target_id, target);
    }

    #[test]
    fn test_query_limit() {
        let (log, _dir) = make_audit_log();
        for _ in 0..10 {
            log.log(make_entry(AuditAction::NodeCreated, "kai"))
                .unwrap();
        }
        let entries = log
            .query(AuditFilter {
                limit: Some(3),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 3);
    }
}
