# Phase C — Access Control & Policies

**Duration:** 2 weeks  
**Dependencies:** Phase A, B complete  
**Goal:** Multi-agent isolation, data lifecycle management, and audit trail.

---

## C1. Namespace-Based Access Control

### Data Model

Every node and edge gains a `namespace` field:

```rust
pub struct Node {
    // ... existing fields ...
    pub namespace: String,  // Default: "default"
}
```

### Namespace Configuration

```toml
[access]
mode = "namespace"  # "open" | "namespace"

[[access.namespaces]]
name = "kai"
agents = ["kai", "dutybound"]
inherit = ["shared"]

[[access.namespaces]]
name = "shared"
agents = ["*"]
write_agents = ["kai"]  # Others can read, only kai writes

[[access.namespaces]]
name = "private-agent-b"
agents = ["agent-b"]
```

### Enforcement

```rust
pub struct AccessContext {
    pub agent_id: String,
    pub namespaces: Vec<String>,  // Resolved from config (own + inherited)
}

impl AccessContext {
    pub fn can_read(&self, namespace: &str) -> bool {
        self.namespaces.contains(&namespace.to_string()) 
            || self.namespaces.contains(&"*".to_string())
    }
    
    pub fn can_write(&self, namespace: &str) -> bool {
        // Check write_agents in config
    }
}
```

All storage queries automatically filter by namespace:
- `list_nodes` → adds namespace filter
- `search` → vector filter includes namespace
- `traverse` → skips nodes outside namespace
- `briefing` → only pulls from accessible namespaces

gRPC requests include `agent_id` in metadata headers. HTTP requests use `X-Agent-Id` header.

### Migration
Existing nodes get `namespace = "default"`. Mode defaults to "open" (no filtering).

### Tests
- Agent A can't read Agent B's private namespace
- Inherited namespaces work (kai reads "shared")
- Wildcard `*` grants read to all
- Write restrictions enforced
- Briefings only include accessible nodes
- Search results filtered by namespace
- "open" mode = no filtering (backward compatible)

---

## C2. Retention Policies

### Configuration

```toml
[retention]
enabled = true
default_ttl_days = 0        # 0 = keep forever
grace_period_days = 7        # Soft-deleted → hard-deleted after grace

[retention.by_kind]
observation = 30
event = 90
fact = 0                     # Forever
decision = 0
pattern = 0

[retention.limits]
max_nodes = 0                # 0 = unlimited
eviction_strategy = "oldest_lowest_importance"
```

### Retention Engine

Runs as part of the auto-linker cycle (every Nth cycle):

```rust
pub struct RetentionEngine<S: Storage> {
    storage: Arc<S>,
    config: RetentionConfig,
}

impl<S: Storage> RetentionEngine<S> {
    pub fn apply(&self, now: DateTime<Utc>) -> Result<RetentionStats> {
        let mut stats = RetentionStats::default();
        
        // 1. TTL expiry: soft-delete nodes past their TTL
        for kind_ttl in &self.config.by_kind {
            let cutoff = now - Duration::days(kind_ttl.ttl_days as i64);
            let expired = self.storage.list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![kind_ttl.kind.clone()])
                    .created_before(cutoff)
            )?;
            for node in expired {
                if !node.deleted {
                    self.storage.delete_node(node.id)?;  // Soft delete
                    stats.soft_deleted += 1;
                }
            }
        }
        
        // 2. Grace period: hard-delete nodes soft-deleted > grace_period ago
        let grace_cutoff = now - Duration::days(self.config.grace_period_days as i64);
        let tombstoned = self.storage.list_nodes(
            NodeFilter::new().include_deleted().created_before(grace_cutoff)
        )?;
        for node in tombstoned {
            if node.deleted {
                self.storage.hard_delete_node(node.id)?;
                stats.hard_deleted += 1;
            }
        }
        
        // 3. Node cap: evict if over limit
        if self.config.max_nodes > 0 {
            let count = self.storage.count_nodes(NodeFilter::new())?;
            if count > self.config.max_nodes as u64 {
                let excess = (count - self.config.max_nodes as u64) as usize;
                let evictable = self.storage.list_nodes(
                    NodeFilter::new()
                        .with_limit(excess)
                        // Sort by oldest + lowest importance
                )?;
                for node in evictable {
                    self.storage.delete_node(node.id)?;
                    stats.evicted += 1;
                }
            }
        }
        
        Ok(stats)
    }
}
```

### hard_delete_node

New storage method that physically removes from all tables (not just tombstone):

```rust
fn hard_delete_node(&self, id: NodeId) -> Result<()> {
    // Remove from: NODES, NODES_BY_KIND, NODES_BY_TAG, NODES_BY_SOURCE
    // Remove all edges to/from this node
    // Remove from vector index
}
```

### Tests
- TTL expiry soft-deletes nodes past their age
- Grace period hard-deletes old tombstones
- Node cap evicts oldest+least important first
- High-importance nodes resist eviction
- Edges to deleted nodes are cleaned up
- Retention respects kind-specific TTLs
- TTL=0 means keep forever

---

## C3. Audit Log

### Storage

Append-only table in redb:

```rust
const AUDIT_LOG: TableDefinition<u128, &[u8]> = TableDefinition::new("audit_log");
// Key: timestamp as u128 (nanoseconds since epoch) for ordering

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub action: AuditAction,
    pub target_id: Uuid,
    pub target_type: String,     // "node" or "edge"
    pub actor: String,           // agent ID or system component
    pub namespace: Option<String>,
    pub details: Option<String>, // JSON diff or description
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AuditAction {
    NodeCreated,
    NodeUpdated,
    NodeDeleted,       // Soft delete
    NodeHardDeleted,   // Retention/grace period
    NodeMerged { into: Uuid },
    EdgeCreated,
    EdgeUpdated,
    EdgeDeleted,
    EdgeDecayed { old_weight: f32, new_weight: f32 },
    BriefingGenerated { agent_id: String },
    SearchPerformed { query: String },
    ConfigChanged { field: String },
}
```

### Audit Writer

```rust
pub struct AuditWriter<S: Storage> {
    storage: Arc<S>,
    enabled: bool,
}

impl<S: Storage> AuditWriter<S> {
    pub fn log(&self, entry: AuditEntry) -> Result<()> {
        if !self.enabled { return Ok(()); }
        // Append to AUDIT_LOG table
    }
}
```

Integrated into all storage mutation methods. Zero overhead when disabled.

### Querying

```
cortex audit                              # Last 50 entries
cortex audit --since 24h                  # Last 24 hours
cortex audit --node 018d5f2a-...          # History of specific node
cortex audit --actor auto-linker          # All auto-linker actions
cortex audit --action NodeCreated --since 1h
cortex audit --format json > audit.json   # Export
```

### Retention for Audit Log

Audit log itself has a retention policy:
```toml
[audit]
enabled = true
retention_days = 90  # Delete audit entries older than 90 days
```

### Tests
- Node create/update/delete generates audit entries
- Edge operations generate audit entries
- Audit query by time range
- Audit query by actor
- Audit query by node ID
- Audit retention cleans old entries
- Disabled audit = zero overhead (no writes)

---

## C4. Encryption at Rest

### Implementation

Transparent encryption layer wrapping the redb file.

```toml
[security]
encryption = false  # Disabled by default
# Key via env: CORTEX_ENCRYPTION_KEY=base64-encoded-32-bytes
```

**Approach:** Encrypt the entire redb file using AES-256-GCM with a file-level nonce. On open, decrypt to a temporary file (or memory-mapped). On close/flush, re-encrypt.

```rust
pub struct EncryptedStorage {
    inner: RedbStorage,       // Operates on decrypted temp file
    encrypted_path: PathBuf,  // The actual encrypted file
    key: [u8; 32],
    temp_path: PathBuf,       // Decrypted working copy
}

impl Drop for EncryptedStorage {
    fn drop(&mut self) {
        // Re-encrypt temp to encrypted_path
        // Securely wipe temp file
    }
}
```

**Key derivation:** `CORTEX_ENCRYPTION_KEY` env var → Argon2id → 256-bit key. This way the env var can be a passphrase, not raw bytes.

**Backup interaction:** `cortex backup --encrypt` uses the same key. Backup metadata records `encrypted: true`.

### Tests
- Open encrypted DB, read/write, close, reopen → data persists
- Wrong key → clear error (not garbage data)
- Missing key env var → clear error
- Backup of encrypted DB produces encrypted backup
- Performance: <10% overhead on read/write operations

---

## Deliverables

1. Namespace-based access control with inheritance
2. Retention engine with per-kind TTLs, grace period, node cap
3. `hard_delete_node` storage method
4. Append-only audit log with query CLI
5. AES-256-GCM encryption at rest
6. All integrated into existing storage/engine layer
7. Backward compatible (defaults to open mode, no encryption, no retention)
