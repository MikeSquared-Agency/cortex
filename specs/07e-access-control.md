# Phase 7E: Access Control & Policies

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires `CortexConfig` (namespace/retention/security config blocks), string `NodeKind`.  
**Weeks:** 5–6  

---

## Overview

Multi-agent isolation via namespaces, automatic data expiry via retention policies, an append-only audit log for every mutation, and AES-256-GCM encryption at rest for the redb database file. These features are gated by configuration — the default `access.mode = "open"` behaviour is fully backward compatible.

---

## Repository Layout

```
crates/cortex-core/src/
  access/
    mod.rs          — Namespace resolution + access checking
    retention.rs    — Retention policy enforcement
    audit.rs        — Audit log (AuditEntry, AuditTable)
  storage/
    encrypted.rs    — AES-256-GCM transparent encryption wrapper
```

```
crates/cortex-server/src/
  cli/
    audit.rs        — cortex audit CLI command (Phase 7B adds the plumbing)
```

---

## Task 1: Namespace Model

### File: `crates/cortex-core/src/access/mod.rs`

Every node gets an optional `namespace` field. Queries automatically filter by the requesting agent's allowed namespaces.

**Configuration** (from Phase 7A `CortexConfig`):
```toml
[access]
mode = "namespace"  # "open" (default) | "namespace" | "rbac"

[[access.namespaces]]
name = "kai"
agents = ["kai", "dutybound"]   # These agents can read/write this namespace
inherit = ["shared"]             # Also inherits from "shared" namespace

[[access.namespaces]]
name = "shared"
agents = ["*"]                   # Everyone can read
write = ["kai"]                  # Only kai can write
```

**Node type update** — add namespace field:

In `crates/cortex-core/src/types.rs`, add to the `Node` struct:
```rust
/// Namespace for multi-agent isolation.
/// None means the node belongs to the global/open namespace.
pub namespace: Option<String>,
```

**Namespace resolver:**
```rust
use crate::config::{AccessConfig, AccessMode};

/// Resolves which namespaces an agent can read/write.
pub struct NamespaceResolver {
    config: AccessConfig,
}

impl NamespaceResolver {
    pub fn new(config: AccessConfig) -> Self {
        Self { config }
    }

    /// Returns the list of namespace names an agent can READ.
    pub fn readable_namespaces(&self, agent_id: &str) -> Vec<String> {
        match self.config.mode.as_str() {
            "open" | "" => vec![],  // No filtering — all nodes visible
            "namespace" => self.resolve_readable(agent_id),
            _ => vec![],
        }
    }

    /// Returns the list of namespace names an agent can WRITE to.
    pub fn writable_namespaces(&self, agent_id: &str) -> Vec<String> {
        match self.config.mode.as_str() {
            "open" | "" => vec![],
            "namespace" => self.resolve_writable(agent_id),
            _ => vec![],
        }
    }

    fn resolve_readable(&self, agent_id: &str) -> Vec<String> {
        let mut readable = Vec::new();
        for ns in &self.config.namespaces {
            let can_read = ns.agents.iter().any(|a| a == "*" || a == agent_id);
            if can_read {
                readable.push(ns.name.clone());
                // Recursively add inherited namespaces
                if let Some(inherited) = &ns.inherit {
                    for inh in inherited {
                        if !readable.contains(inh) {
                            readable.push(inh.clone());
                        }
                    }
                }
            }
        }
        readable
    }

    fn resolve_writable(&self, agent_id: &str) -> Vec<String> {
        let mut writable = Vec::new();
        for ns in &self.config.namespaces {
            let can_write = match &ns.write {
                Some(writers) => writers.iter().any(|a| a == "*" || a == agent_id),
                None => ns.agents.iter().any(|a| a == "*" || a == agent_id),
            };
            if can_write {
                writable.push(ns.name.clone());
            }
        }
        writable
    }

    /// Resolve the default namespace for a new node created by agent_id.
    pub fn default_namespace_for(&self, agent_id: &str) -> Option<String> {
        match self.config.mode.as_str() {
            "open" | "" => None,
            "namespace" => {
                // The first namespace the agent can write to
                self.resolve_writable(agent_id).into_iter().next()
            }
            _ => None,
        }
    }
}
```

**Storage filter integration:**

In `crates/cortex-core/src/storage/filters.rs`, add namespace filtering to `NodeFilter`:
```rust
pub struct NodeFilter {
    // ... existing fields ...
    /// If Some, only return nodes in these namespaces.
    /// If None, no namespace filter applied.
    pub namespaces: Option<Vec<String>>,
}

impl NodeFilter {
    pub fn with_namespaces(mut self, namespaces: Vec<String>) -> Self {
        self.namespaces = Some(namespaces);
        self
    }
}
```

**gRPC service integration:**

In `crates/cortex-server/src/grpc/service.rs`, every request that reads nodes must be filtered through the namespace resolver:

```rust
// Before returning nodes, filter by agent's readable namespaces:
let readable = resolver.readable_namespaces(&agent_id);
let filter = if readable.is_empty() {
    base_filter  // open mode: no namespace filter
} else {
    base_filter.with_namespaces(readable)
};
```

The `agent_id` comes from the gRPC request metadata header: `x-cortex-agent-id`.

---

## Task 2: Retention Policies

### File: `crates/cortex-core/src/access/retention.rs`

Automatic node expiry based on kind-specific TTLs and total node count limits.

**Configuration** (from Phase 7A `CortexConfig`):
```toml
[retention]
default_ttl_days = 0          # 0 = keep forever

[retention.by_kind]
observation = 30              # Expire observations after 30 days
event = 90                    # Events after 90 days
decision = 0                  # Decisions kept forever
pattern = 0                   # Patterns kept forever

[retention.max_nodes]
limit = 100000                # Hard cap on total nodes
strategy = "oldest_lowest_importance"  # Eviction strategy
```

**Retention engine:**
```rust
use crate::{Storage, NodeFilter, CortexError, Result};
use crate::config::RetentionConfig;
use chrono::{Utc, Duration};

pub struct RetentionEngine {
    config: RetentionConfig,
}

impl RetentionEngine {
    pub fn new(config: RetentionConfig) -> Self {
        Self { config }
    }

    /// Run a retention sweep. Called by the auto-linker's background loop.
    /// Returns the number of nodes soft-deleted.
    pub fn sweep(&self, storage: &impl Storage) -> Result<usize> {
        let mut deleted = 0;

        // 1. TTL-based expiry
        let now = Utc::now();

        for (kind_str, ttl_days) in &self.config.by_kind {
            if *ttl_days == 0 { continue; }

            let cutoff = now - Duration::days(*ttl_days as i64);
            let expired = storage.list_nodes(
                NodeFilter::new()
                    .with_kind(kind_str.clone())
                    .with_created_before(cutoff)
                    .with_not_deleted(),
            )?;

            for node in expired {
                storage.soft_delete_node(node.id)?;
                deleted += 1;
            }
        }

        // Also apply default TTL to all kinds not explicitly listed
        if self.config.default_ttl_days > 0 {
            let cutoff = now - Duration::days(self.config.default_ttl_days as i64);
            let expired = storage.list_nodes(
                NodeFilter::new()
                    .with_created_before(cutoff)
                    .with_not_deleted(),
            )?;
            for node in expired {
                // Skip kinds with explicit 0 TTL (kept forever)
                let kind_str = node.kind.as_str().to_string();
                if self.config.by_kind.get(&kind_str).copied() == Some(0) {
                    continue;
                }
                storage.soft_delete_node(node.id)?;
                deleted += 1;
            }
        }

        // 2. Max node cap
        if let Some(max_cfg) = &self.config.max_nodes {
            let stats = storage.stats()?;
            if stats.total_nodes > max_cfg.limit {
                let excess = stats.total_nodes - max_cfg.limit;
                let to_evict = self.select_eviction_candidates(storage, excess, &max_cfg.strategy)?;
                for id in to_evict {
                    storage.soft_delete_node(id)?;
                    deleted += 1;
                }
            }
        }

        Ok(deleted)
    }

    /// Select nodes to evict based on strategy.
    fn select_eviction_candidates(
        &self,
        storage: &impl Storage,
        count: usize,
        strategy: &str,
    ) -> Result<Vec<uuid::Uuid>> {
        match strategy {
            "oldest_lowest_importance" => {
                // Sort by (importance ASC, created_at ASC), take `count`
                let mut nodes = storage.list_nodes(NodeFilter::new().with_not_deleted())?;
                nodes.sort_by(|a, b| {
                    a.importance.partial_cmp(&b.importance).unwrap()
                        .then(a.created_at.cmp(&b.created_at))
                });
                Ok(nodes.into_iter().take(count).map(|n| n.id).collect())
            }
            _ => Err(CortexError::Validation(format!("Unknown eviction strategy: {}", strategy))),
        }
    }
}
```

**NodeFilter additions needed** (in `crates/cortex-core/src/storage/filters.rs`):
```rust
pub fn with_created_before(mut self, cutoff: chrono::DateTime<chrono::Utc>) -> Self {
    self.created_before = Some(cutoff);
    self
}
pub fn with_not_deleted(mut self) -> Self {
    self.include_deleted = false;
    self
}
```

**Soft-delete vs hard-delete:**
- Soft delete: sets `node.deleted = true`, node remains in storage but excluded from queries.
- Hard delete: runs after a **grace period of 7 days** (configurable). The auto-linker's cleanup pass permanently removes nodes that have been soft-deleted for longer than the grace period.

---

## Task 3: Audit Log

### File: `crates/cortex-core/src/access/audit.rs`

An append-only log of every mutation, stored in a dedicated redb table.

**Audit entry struct:**
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When this action occurred.
    pub timestamp: DateTime<Utc>,
    /// What happened.
    pub action: AuditAction,
    /// The node or edge that was affected.
    pub target_id: Uuid,
    /// Who caused this action.
    pub actor: String,
    /// JSON diff or human-readable description. Optional.
    pub details: Option<String>,
}

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
            AuditAction::NodeCreated     => write!(f, "node.created"),
            AuditAction::NodeUpdated     => write!(f, "node.updated"),
            AuditAction::NodeDeleted     => write!(f, "node.deleted"),
            AuditAction::NodeHardDeleted => write!(f, "node.hard_deleted"),
            AuditAction::EdgeCreated     => write!(f, "edge.created"),
            AuditAction::EdgeDecayed     => write!(f, "edge.decayed"),
            AuditAction::EdgePruned      => write!(f, "edge.pruned"),
            AuditAction::NodeMerged      => write!(f, "node.merged"),
            AuditAction::BriefingGenerated => write!(f, "briefing.generated"),
            AuditAction::SchemaUpgraded  => write!(f, "schema.upgraded"),
        }
    }
}
```

**Audit table in redb:**

The audit table is append-only. Entries are keyed by `(timestamp_nanos as u128, target_id as bytes)` for time-ordered iteration.

```rust
use redb::{TableDefinition, Database};

const AUDIT_TABLE: TableDefinition<u128, &[u8]> = TableDefinition::new("audit");

pub struct AuditLog {
    db: std::sync::Arc<Database>,
}

impl AuditLog {
    pub fn new(db: std::sync::Arc<Database>) -> Self {
        Self { db }
    }

    /// Append an audit entry. This is a write to the redb audit table.
    pub fn log(&self, entry: AuditEntry) -> crate::Result<()> {
        let key = entry.timestamp.timestamp_nanos_opt().unwrap_or(0) as u128;
        let value = serde_json::to_vec(&entry)
            .map_err(|e| crate::CortexError::Validation(format!("Audit serialise: {}", e)))?;

        let write_txn = self.db.begin_write()
            .map_err(|e| crate::CortexError::Validation(format!("Audit write: {}", e)))?;
        {
            let mut table = write_txn.open_table(AUDIT_TABLE)
                .map_err(|e| crate::CortexError::Validation(format!("Audit table: {}", e)))?;
            table.insert(key, value.as_slice())
                .map_err(|e| crate::CortexError::Validation(format!("Audit insert: {}", e)))?;
        }
        write_txn.commit()
            .map_err(|e| crate::CortexError::Validation(format!("Audit commit: {}", e)))?;
        Ok(())
    }

    /// Query audit entries.
    pub fn query(&self, filter: AuditFilter) -> crate::Result<Vec<AuditEntry>> {
        let read_txn = self.db.begin_read()
            .map_err(|e| crate::CortexError::Validation(format!("Audit read: {}", e)))?;
        let table = read_txn.open_table(AUDIT_TABLE)
            .map_err(|e| crate::CortexError::Validation(format!("Audit table: {}", e)))?;

        let since_nanos = filter.since
            .map(|t| t.timestamp_nanos_opt().unwrap_or(0) as u128)
            .unwrap_or(0);

        let mut entries = Vec::new();
        for result in table.range(since_nanos..)
            .map_err(|e| crate::CortexError::Validation(format!("Audit range: {}", e)))?
        {
            let (_, value) = result
                .map_err(|e| crate::CortexError::Validation(format!("Audit iter: {}", e)))?;
            if let Ok(entry) = serde_json::from_slice::<AuditEntry>(value.value()) {
                // Apply filters
                if let Some(ref actor) = filter.actor {
                    if entry.actor != *actor { continue; }
                }
                if let Some(ref node_id) = filter.node_id {
                    if entry.target_id != *node_id { continue; }
                }
                if let Some(ref action) = filter.action {
                    if entry.action != *action { continue; }
                }
                entries.push(entry);
                if let Some(limit) = filter.limit {
                    if entries.len() >= limit { break; }
                }
            }
        }

        Ok(entries)
    }
}

#[derive(Debug, Default)]
pub struct AuditFilter {
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub actor: Option<String>,
    pub node_id: Option<uuid::Uuid>,
    pub action: Option<AuditAction>,
    pub limit: Option<usize>,
}
```

**Integration — log every mutation:**

In `crates/cortex-core/src/storage/redb_storage.rs`, wrap all write operations to emit audit entries:

```rust
// In RedbStorage, add an optional audit_log field:
pub struct RedbStorage {
    db: Arc<Database>,
    audit_log: Option<Arc<AuditLog>>,
}

impl RedbStorage {
    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    fn audit(&self, entry: AuditEntry) {
        if let Some(ref log) = self.audit_log {
            if let Err(e) = log.log(entry) {
                tracing::error!("Audit log write failed: {}", e);
            }
        }
    }
}

// Example: wrap put_node
pub fn put_node(&self, node: &Node) -> Result<()> {
    let is_new = self.get_node(node.id)?.is_none();
    // ... existing write logic ...
    self.audit(AuditEntry {
        timestamp: Utc::now(),
        action: if is_new { AuditAction::NodeCreated } else { AuditAction::NodeUpdated },
        target_id: node.id,
        actor: node.source.agent.clone(),
        details: None,
    });
    Ok(())
}
```

**CLI command** (extends Phase 7B CLI):

### File: `crates/cortex-server/src/cli/audit.rs`

```
$ cortex audit --since 24h

2026-01-20T10:23:41Z  node.created   [abc123]  actor: kai
2026-01-20T10:23:42Z  edge.created   [def456]  actor: auto-linker
2026-01-20T10:24:01Z  node.updated   [abc123]  actor: kai
2026-01-20T10:25:00Z  edge.decayed   [def456]  actor: decay-engine

$ cortex audit --node abc123
$ cortex audit --actor auto-linker --since 1h
$ cortex audit --format json --since 24h
```

```rust
#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Only show entries since this duration (e.g. "24h", "7d")
    #[arg(long)]
    pub since: Option<String>,
    /// Filter by node ID
    #[arg(long)]
    pub node: Option<String>,
    /// Filter by actor
    #[arg(long)]
    pub actor: Option<String>,
    /// Output format: table (default) | json
    #[arg(long, default_value = "table")]
    pub format: String,
    #[arg(long, default_value = "100")]
    pub limit: usize,
}
```

---

## Task 4: Encryption at Rest

### File: `crates/cortex-core/src/storage/encrypted.rs`

AES-256-GCM encryption of the redb file via a transparent wrapper layer.

**Configuration:**
```toml
[security]
encryption = true
# CORTEX_ENCRYPTION_KEY env var must be set (base64-encoded 256-bit key)
# Key is NEVER stored in the config file.
```

**Key derivation:**

The encryption key is derived from the `CORTEX_ENCRYPTION_KEY` environment variable using Argon2id:

```rust
use argon2::{Argon2, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// Derive a 256-bit AES key from the CORTEX_ENCRYPTION_KEY env var.
/// Uses Argon2id with a fixed salt derived from the database path.
pub fn derive_key(db_path: &std::path::Path) -> anyhow::Result<[u8; 32]> {
    let raw_key = std::env::var("CORTEX_ENCRYPTION_KEY")
        .map_err(|_| anyhow::anyhow!("CORTEX_ENCRYPTION_KEY environment variable not set"))?;

    let key_bytes = BASE64.decode(&raw_key)
        .map_err(|_| anyhow::anyhow!("CORTEX_ENCRYPTION_KEY is not valid base64"))?;

    if key_bytes.len() != 32 {
        return Err(anyhow::anyhow!("CORTEX_ENCRYPTION_KEY must be a 256-bit (32-byte) key"));
    }

    let mut output = [0u8; 32];
    output.copy_from_slice(&key_bytes);
    Ok(output)
}
```

**Encryption implementation:**

Encryption is applied to the entire redb file using a streaming AES-256-GCM approach. Because redb manages its own file, we use a **pre-open hook**:
- On open: if encryption is enabled, decrypt the file to a temp location, open the temp file with redb, then on close re-encrypt to the original path.
- On backup: the backup is also encrypted.

**Cargo.toml dependencies:**
```toml
aes-gcm = "0.10"
argon2 = "0.5"
base64 = "0.22"
```

```rust
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};

/// Encrypt a file in-place using AES-256-GCM.
/// A random 96-bit nonce is prepended to the ciphertext.
pub fn encrypt_file(path: &std::path::Path, key: &[u8; 32]) -> anyhow::Result<()> {
    let plaintext = std::fs::read(path)?;
    let cipher = Aes256Gcm::new(key.into());

    // Generate random nonce
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plaintext.as_ref())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    let mut output = nonce_bytes.to_vec();
    output.extend(ciphertext);

    std::fs::write(path, output)?;
    Ok(())
}

/// Decrypt a file in-place using AES-256-GCM.
pub fn decrypt_file(path: &std::path::Path, key: &[u8; 32]) -> anyhow::Result<()> {
    let data = std::fs::read(path)?;

    if data.len() < 12 {
        return Err(anyhow::anyhow!("File too short to be encrypted"));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key.into());

    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

    std::fs::write(path, plaintext)?;
    Ok(())
}
```

**Startup flow when encryption is enabled:**

```rust
// In cortex-server/src/main.rs or serve::run():
if config.security.encryption {
    let key = encrypted::derive_key(&db_path)?;
    // Decrypt to temp file
    let temp_path = db_path.with_extension("redb.tmp");
    std::fs::copy(&db_path, &temp_path)?;
    encrypted::decrypt_file(&temp_path, &key)?;
    // Open redb against temp file
    let storage = RedbStorage::open(&temp_path)?;
    // On shutdown: re-encrypt and replace original
    // (register shutdown hook)
}
```

**Backup encryption:** The `cortex backup --encrypt` flag (Phase 7B) calls `encrypt_file()` on the backup copy.

**Key generation helper:**
```
$ cortex security generate-key

Generated a new 256-bit encryption key.
Add to your environment:
  export CORTEX_ENCRYPTION_KEY="<base64-encoded key>"

Keep this key safe — data encrypted with it cannot be recovered without it.
```

---

## Definition of Done

- [ ] With `[access] mode = "open"`, all existing behaviour is unchanged
- [ ] With `[access] mode = "namespace"`, nodes have a `namespace` field set on create
- [ ] Namespace resolver correctly identifies which namespaces an agent can read
- [ ] Namespace resolver correctly identifies which namespaces an agent can write to
- [ ] `agents = ["*"]` grants read access to all agents
- [ ] Namespace inheritance: agent in `"kai"` that inherits `"shared"` can read both
- [ ] Queries via gRPC filter nodes by the requesting agent's `x-cortex-agent-id` header
- [ ] `NodeFilter::with_namespaces()` correctly restricts `list_nodes()` results
- [ ] Retention engine soft-deletes nodes past their TTL when `sweep()` is called
- [ ] Retention engine respects `by_kind` per-kind TTLs
- [ ] Retention engine skips kinds with explicit `ttl = 0` (keep forever) during default TTL sweep
- [ ] Retention engine evicts oldest/least-important nodes when `max_nodes.limit` is exceeded
- [ ] Hard delete runs after grace period (7 days by default)
- [ ] Auto-linker's background loop calls `RetentionEngine::sweep()` on each cycle
- [ ] `AuditLog::log()` appends entries to the redb `audit` table
- [ ] `AuditLog::query()` returns entries filtered by `since`, `actor`, `node_id`
- [ ] Every `put_node()` call emits a `NodeCreated` or `NodeUpdated` audit entry
- [ ] Every `soft_delete_node()` call emits a `NodeDeleted` audit entry
- [ ] Every `put_edge()` call emits an `EdgeCreated` audit entry
- [ ] `cortex audit --since 24h` prints the last 24h of audit entries
- [ ] `cortex audit --node <id>` shows all audit entries for a specific node
- [ ] `cortex audit --actor auto-linker` filters to auto-linker actions
- [ ] `cortex audit --format json` outputs JSON array of entries
- [ ] With `[security] encryption = true` and `CORTEX_ENCRYPTION_KEY` set, database is encrypted on disk
- [ ] Without `CORTEX_ENCRYPTION_KEY`, server refuses to start with a clear error
- [ ] `CORTEX_ENCRYPTION_KEY` that is not 32 bytes (base64-decoded) is rejected with a clear error
- [ ] Encrypted database cannot be opened by redb directly (ciphertext, not valid redb format)
- [ ] `cortex backup --encrypt ./backup.redb` creates an encrypted backup
- [ ] `cortex security generate-key` outputs a valid base64-encoded 32-byte key
- [ ] `cargo test --workspace` passes with all access control features
