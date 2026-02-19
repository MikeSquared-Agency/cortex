use crate::error::{CortexError, Result};
use crate::storage::filters::{NodeFilter, StorageStats};
use crate::storage::traits::Storage;
use crate::types::{Edge, EdgeId, Node, NodeId};
use chrono::{DateTime, Utc};
use redb::{Database, MultimapTableDefinition, ReadableMultimapTable, ReadableTable, TableDefinition};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Table definitions
const NODES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("nodes");
const EDGES: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("edges");

// Secondary indexes (v2: kind stored as &str, not u8)
const NODES_BY_KIND: MultimapTableDefinition<&str, &[u8; 16]> =
    MultimapTableDefinition::new("nodes_by_kind_v2");
const EDGES_BY_FROM: MultimapTableDefinition<&[u8; 16], &[u8; 16]> =
    MultimapTableDefinition::new("edges_by_from");
const EDGES_BY_TO: MultimapTableDefinition<&[u8; 16], &[u8; 16]> =
    MultimapTableDefinition::new("edges_by_to");
const NODES_BY_TAG: MultimapTableDefinition<&str, &[u8; 16]> =
    MultimapTableDefinition::new("nodes_by_tag");
const NODES_BY_SOURCE: MultimapTableDefinition<&str, &[u8; 16]> =
    MultimapTableDefinition::new("nodes_by_source");

// Metadata table
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

/// Current schema version.
/// v1 = original (NodeKind stored as u8 in nodes_by_kind)
/// v2 = string-based NodeKind/Relation, nodes_by_kind_v2 table
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
const SCHEMA_VERSION_KEY: &str = "schema_version";
const STATS_NODE_COUNT_KEY: &str = "stats:node_count";
const STATS_EDGE_COUNT_KEY: &str = "stats:edge_count";

/// Redb-based storage implementation
pub struct RedbStorage {
    db: Arc<Database>,
    #[allow(dead_code)]
    path: PathBuf,
}

impl RedbStorage {
    /// Open or create a database at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CortexError::Validation(format!("Failed to create directory: {}", e))
            })?;
        }

        let is_new = !path.exists();
        let db = Database::create(&path)?;

        if is_new {
            // New database: initialize all tables and write schema version
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(NODES)?;
                let _ = write_txn.open_table(EDGES)?;
                let _ = write_txn.open_multimap_table(NODES_BY_KIND)?;
                let _ = write_txn.open_multimap_table(EDGES_BY_FROM)?;
                let _ = write_txn.open_multimap_table(EDGES_BY_TO)?;
                let _ = write_txn.open_multimap_table(NODES_BY_TAG)?;
                let _ = write_txn.open_multimap_table(NODES_BY_SOURCE)?;
                let mut meta = write_txn.open_table(META)?;
                meta.insert(SCHEMA_VERSION_KEY, CURRENT_SCHEMA_VERSION.to_string().as_bytes())?;
            }
            write_txn.commit()?;
        } else {
            // Existing database: check schema version
            Self::check_schema_version(&db)?;
            // Ensure tables exist
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(NODES)?;
                let _ = write_txn.open_table(EDGES)?;
                let _ = write_txn.open_multimap_table(NODES_BY_KIND)?;
                let _ = write_txn.open_multimap_table(EDGES_BY_FROM)?;
                let _ = write_txn.open_multimap_table(EDGES_BY_TO)?;
                let _ = write_txn.open_multimap_table(NODES_BY_TAG)?;
                let _ = write_txn.open_multimap_table(NODES_BY_SOURCE)?;
                let _ = write_txn.open_table(META)?;
            }
            write_txn.commit()?;
        }

        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    /// Check schema version. Returns error if migration is needed.
    fn check_schema_version(db: &Database) -> Result<()> {
        let read_txn = db.begin_read()?;
        let version = {
            let table = read_txn.open_table(META).ok();
            table
                .and_then(|t| {
                    t.get(SCHEMA_VERSION_KEY).ok().flatten().and_then(|v| {
                        std::str::from_utf8(v.value())
                            .ok()
                            .and_then(|s| s.parse::<u32>().ok())
                    })
                })
                .unwrap_or(1) // No version entry = v1
        };

        match version.cmp(&CURRENT_SCHEMA_VERSION) {
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Less => Err(CortexError::Validation(format!(
                "Database schema v{} is older than current v{}. Run `cortex migrate`.",
                version, CURRENT_SCHEMA_VERSION
            ))),
            std::cmp::Ordering::Greater => Err(CortexError::Validation(format!(
                "Database schema v{} is newer than this binary v{}. Upgrade Cortex.",
                version, CURRENT_SCHEMA_VERSION
            ))),
        }
    }

    /// Get the database file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Helper to convert UUID to byte array
    fn uuid_to_bytes(id: &uuid::Uuid) -> [u8; 16] {
        *id.as_bytes()
    }

    /// Helper to convert byte array to UUID
    fn bytes_to_uuid(bytes: &[u8; 16]) -> uuid::Uuid {
        uuid::Uuid::from_bytes(*bytes)
    }

    /// Serialize a node to bytes
    fn serialize_node(node: &Node) -> Result<Vec<u8>> {
        bincode::serialize(node).map_err(CortexError::from)
    }

    /// Deserialize a node from bytes
    fn deserialize_node(bytes: &[u8]) -> Result<Node> {
        bincode::deserialize(bytes).map_err(CortexError::from)
    }

    /// Public helper for migration: attempt to deserialize a node from raw bytes.
    pub fn try_deserialize_node(bytes: &[u8]) -> Result<Node> {
        Self::deserialize_node(bytes)
    }

    /// Serialize an edge to bytes
    fn serialize_edge(edge: &Edge) -> Result<Vec<u8>> {
        bincode::serialize(edge).map_err(CortexError::from)
    }

    /// Deserialize an edge from bytes
    fn deserialize_edge(bytes: &[u8]) -> Result<Edge> {
        bincode::deserialize(bytes).map_err(CortexError::from)
    }

    /// Update secondary indexes for a node
    fn update_node_indexes(
        &self,
        txn: &redb::WriteTransaction,
        node: &Node,
        old_node: Option<&Node>,
    ) -> Result<()> {
        let node_id_bytes = Self::uuid_to_bytes(&node.id);

        // Update kind index
        {
            let mut kind_table = txn.open_multimap_table(NODES_BY_KIND)?;

            // Remove old kind if node existed before
            if let Some(old) = old_node {
                kind_table.remove(old.kind.as_str(), &node_id_bytes)?;
            }

            kind_table.insert(node.kind.as_str(), &node_id_bytes)?;
        }

        // Update source index
        {
            let mut source_table = txn.open_multimap_table(NODES_BY_SOURCE)?;

            // Remove old source if changed
            if let Some(old) = old_node {
                if old.source.agent != node.source.agent {
                    source_table.remove(old.source.agent.as_str(), &node_id_bytes)?;
                }
            }

            source_table.insert(node.source.agent.as_str(), &node_id_bytes)?;
        }

        // Update tag index
        {
            let mut tag_table = txn.open_multimap_table(NODES_BY_TAG)?;

            // Remove old tags if they changed
            if let Some(old) = old_node {
                for old_tag in &old.data.tags {
                    if !node.data.tags.contains(old_tag) {
                        tag_table.remove(old_tag.as_str(), &node_id_bytes)?;
                    }
                }
            }

            // Add new tags
            for tag in &node.data.tags {
                tag_table.insert(tag.as_str(), &node_id_bytes)?;
            }
        }

        Ok(())
    }

    /// Update edge indexes
    fn update_edge_indexes(&self, txn: &redb::WriteTransaction, edge: &Edge) -> Result<()> {
        let edge_id_bytes = Self::uuid_to_bytes(&edge.id);
        let from_bytes = Self::uuid_to_bytes(&edge.from);
        let to_bytes = Self::uuid_to_bytes(&edge.to);

        {
            let mut from_table = txn.open_multimap_table(EDGES_BY_FROM)?;
            from_table.insert(&from_bytes, &edge_id_bytes)?;
        }

        {
            let mut to_table = txn.open_multimap_table(EDGES_BY_TO)?;
            to_table.insert(&to_bytes, &edge_id_bytes)?;
        }

        Ok(())
    }

    /// Remove edge from indexes
    fn remove_edge_from_indexes(&self, txn: &redb::WriteTransaction, edge: &Edge) -> Result<()> {
        let edge_id_bytes = Self::uuid_to_bytes(&edge.id);
        let from_bytes = Self::uuid_to_bytes(&edge.from);
        let to_bytes = Self::uuid_to_bytes(&edge.to);

        {
            let mut from_table = txn.open_multimap_table(EDGES_BY_FROM)?;
            from_table.remove(&from_bytes, &edge_id_bytes)?;
        }

        {
            let mut to_table = txn.open_multimap_table(EDGES_BY_TO)?;
            to_table.remove(&to_bytes, &edge_id_bytes)?;
        }

        Ok(())
    }

    /// Check if a node matches the filter criteria
    fn node_matches_filter(node: &Node, filter: &NodeFilter) -> bool {
        // Check deleted flag
        if !filter.include_deleted && node.deleted {
            return false;
        }

        // Check kind
        if let Some(ref kinds) = filter.kinds {
            if !kinds.contains(&node.kind) {
                return false;
            }
        }

        // Check tags (node must have at least one of the filter tags)
        if let Some(ref tags) = filter.tags {
            if !tags.iter().any(|t| node.data.tags.contains(t)) {
                return false;
            }
        }

        // Check source agent
        if let Some(ref agent) = filter.source_agent {
            if node.source.agent != *agent {
                return false;
            }
        }

        // Check time range
        if let Some(after) = filter.created_after {
            if node.created_at < after {
                return false;
            }
        }

        if let Some(before) = filter.created_before {
            if node.created_at > before {
                return false;
            }
        }

        // Check importance
        if let Some(min_importance) = filter.min_importance {
            if node.importance < min_importance {
                return false;
            }
        }

        true
    }


    fn increment_meta_counter(&self, key: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(META)?;
            let current = meta.get(key)?.map(|v| {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(v.value());
                u64::from_le_bytes(bytes)
            }).unwrap_or(0);
            meta.insert(key, (current + 1).to_le_bytes().as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn decrement_meta_counter(&self, key: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(META)?;
            let current = meta.get(key)?.map(|v| {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(v.value());
                u64::from_le_bytes(bytes)
            }).unwrap_or(0);
            meta.insert(key, current.saturating_sub(1).to_le_bytes().as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn read_meta_counter(&self, key: &str) -> Result<Option<u64>> {
        let read_txn = self.db.begin_read()?;
        let meta = read_txn.open_table(META)?;
        Ok(meta.get(key)?.map(|v| {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(v.value());
            u64::from_le_bytes(bytes)
        }))
    }


}

impl Storage for RedbStorage {
    fn put_node(&self, node: &Node) -> Result<()> {
        // Validate node
        node.validate()
            .map_err(|e| CortexError::Validation(e))?;

        let write_txn = self.db.begin_write()?;

        // Check if node already exists to get old version
        let node_id_bytes = Self::uuid_to_bytes(&node.id);
        let old_node = {
            let nodes_table = write_txn.open_table(NODES)?;
            let old_bytes = nodes_table.get(&node_id_bytes)?.map(|guard| guard.value().to_vec());
            old_bytes.map(|bytes| Self::deserialize_node(&bytes)).transpose()?
        };

        // Serialize and store node
        let node_bytes = Self::serialize_node(node)?;
        {
            let mut nodes_table = write_txn.open_table(NODES)?;
            let node_id_bytes = Self::uuid_to_bytes(&node.id);
            nodes_table.insert(&node_id_bytes, node_bytes.as_slice())?;
        }

        // Update indexes
        self.update_node_indexes(&write_txn, node, old_node.as_ref())?;

        write_txn.commit()?;

        // Increment node count for new nodes
        if old_node.is_none() {
            self.increment_meta_counter(STATS_NODE_COUNT_KEY)?;
        }

        Ok(())
    }

    fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(NODES)?;
        let id_bytes = Self::uuid_to_bytes(&id);

        if let Some(bytes) = table.get(&id_bytes)? {
            let node = Self::deserialize_node(bytes.value())?;
            Ok(Some(node))
        } else {
            Ok(None)
        }
    }

    fn delete_node(&self, id: NodeId) -> Result<()> {
        let mut node = self
            .get_node(id)?
            .ok_or(CortexError::NodeNotFound(id))?;

        node.deleted = true;
        node.updated_at = Utc::now();

        // put_node won't increment (node already exists), decrement manually
        self.put_node(&node)?;
        self.decrement_meta_counter(STATS_NODE_COUNT_KEY)?;
        Ok(())
    }

    fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>> {
        let read_txn = self.db.begin_read()?;
        let nodes_table = read_txn.open_table(NODES)?;

        let mut nodes = Vec::new();

        // If we have a kind filter, use the index for efficiency
        if let Some(ref kinds) = filter.kinds {
            let kind_index = read_txn.open_multimap_table(NODES_BY_KIND)?;

            for kind in kinds {
                let node_ids: Vec<NodeId> = kind_index
                    .get(kind.as_str())?
                    .map(|result| result.map(|guard| Self::bytes_to_uuid(guard.value())))
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                for node_id in node_ids {
                    let node_id_bytes = Self::uuid_to_bytes(&node_id);
                    if let Some(bytes) = nodes_table.get(&node_id_bytes)? {
                        let node = Self::deserialize_node(bytes.value())?;
                        if Self::node_matches_filter(&node, &filter) {
                            nodes.push(node);
                            // Early exit when limit reached (no offset case)
                            if filter.offset.is_none() {
                                if let Some(limit) = filter.limit {
                                    if nodes.len() >= limit {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Full table scan
            for item in nodes_table.iter()? {
                let (_, value) = item?;
                let node = Self::deserialize_node(value.value())?;
                if Self::node_matches_filter(&node, &filter) {
                    nodes.push(node);
                    // Early exit: if no offset, stop once limit is reached
                    if filter.offset.is_none() {
                        if let Some(limit) = filter.limit {
                            if nodes.len() >= limit {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Sort by created_at descending (newest first)
        nodes.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply offset and limit
        let start = filter.offset.unwrap_or(0);
        let end = filter.limit.map(|l| start + l).unwrap_or(nodes.len());
        Ok(nodes.into_iter().skip(start).take(end - start).collect())
    }

    fn count_nodes(&self, filter: NodeFilter) -> Result<u64> {
        // Optimized: count without materializing full Node structs
        // For simple kind-only filters, use the index directly
        if filter.tags.is_none()
            && filter.source_agent.is_none()
            && filter.created_after.is_none()
            && filter.created_before.is_none()
            && filter.min_importance.is_none()
            && !filter.include_deleted
        {
            if let Some(ref kinds) = filter.kinds {
                let read_txn = self.db.begin_read()?;
                let kind_index = read_txn.open_multimap_table(NODES_BY_KIND)?;
                let mut count = 0u64;
                for kind in kinds {
                    count += kind_index.get(kind.as_str())?.count() as u64;
                }
                return Ok(count);
            }
        }
        // Fallback: materialize and count
        Ok(self.list_nodes(filter)?.len() as u64)
    }

    fn put_edge(&self, edge: &Edge) -> Result<()> {
        // Validate edge
        edge.validate()
            .map_err(|e| CortexError::Validation(e))?;

        let from_bytes = Self::uuid_to_bytes(&edge.from);
        let to_bytes = Self::uuid_to_bytes(&edge.to);
        let edge_id_bytes = Self::uuid_to_bytes(&edge.id);

        // Single write transaction: validate nodes, check duplicates, write — all atomic
        let write_txn = self.db.begin_write()?;

        // 1. Check source and target nodes exist and are not deleted
        {
            let nodes_table = write_txn.open_table(NODES)?;

            let from_data = nodes_table.get(&from_bytes)?
                .ok_or_else(|| CortexError::InvalidEdge {
                    reason: format!("Source node {} does not exist", edge.from),
                })?;
            let from_node: Node = Self::deserialize_node(from_data.value())?;
            if from_node.deleted {
                return Err(CortexError::InvalidEdge {
                    reason: format!("Source node {} is deleted", edge.from),
                });
            }

            let to_data = nodes_table.get(&to_bytes)?
                .ok_or_else(|| CortexError::InvalidEdge {
                    reason: format!("Target node {} does not exist", edge.to),
                })?;
            let to_node: Node = Self::deserialize_node(to_data.value())?;
            if to_node.deleted {
                return Err(CortexError::InvalidEdge {
                    reason: format!("Target node {} is deleted", edge.to),
                });
            }
        } // nodes_table dropped

        // 2. Collect existing outgoing edge IDs from the from-index
        // Copy the raw bytes eagerly so they outlive the table handle
        let existing_edge_ids: Vec<EdgeId> = {
            let from_index = write_txn.open_multimap_table(EDGES_BY_FROM)?;
            let raw: Vec<[u8; 16]> = from_index
                .get(&from_bytes)?
                .map(|r| r.map(|g| *g.value()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            raw.into_iter().map(|b| Self::bytes_to_uuid(&b)).collect()
        }; // from_index dropped

        // 3. Check for duplicates (same from + to + relation, different id)
        {
            let edges_table = write_txn.open_table(EDGES)?;
            for eid in &existing_edge_ids {
                let eid_bytes = Self::uuid_to_bytes(eid);
                if let Some(bytes) = edges_table.get(&eid_bytes)? {
                    let existing: Edge = Self::deserialize_edge(bytes.value())?;
                    if existing.to == edge.to
                        && existing.relation == edge.relation
                        && existing.id != edge.id
                    {
                        return Err(CortexError::DuplicateEdge {
                            from: edge.from,
                            to: edge.to,
                            relation: edge.relation.to_string(),
                        });
                    }
                }
            }
        } // edges_table dropped

        // 4. Write the edge
        let edge_bytes = Self::serialize_edge(edge)?;
        {
            let mut edges_table = write_txn.open_table(EDGES)?;
            edges_table.insert(&edge_id_bytes, edge_bytes.as_slice())?;
        } // edges_table dropped

        // 5. Update indexes (reopens EDGES_BY_FROM and EDGES_BY_TO — safe after drop above)
        self.update_edge_indexes(&write_txn, edge)?;

        write_txn.commit()?;
        self.increment_meta_counter(STATS_EDGE_COUNT_KEY)?;
        Ok(())
    }

    fn get_edge(&self, id: EdgeId) -> Result<Option<Edge>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EDGES)?;
        let id_bytes = Self::uuid_to_bytes(&id);

        if let Some(bytes) = table.get(&id_bytes)? {
            let edge = Self::deserialize_edge(bytes.value())?;
            Ok(Some(edge))
        } else {
            Ok(None)
        }
    }

    fn delete_edge(&self, id: EdgeId) -> Result<()> {
        let edge = self
            .get_edge(id)?
            .ok_or(CortexError::EdgeNotFound(id))?;

        let write_txn = self.db.begin_write()?;

        // Remove from indexes first
        self.remove_edge_from_indexes(&write_txn, &edge)?;

        // Remove from main table
        {
            let mut edges_table = write_txn.open_table(EDGES)?;
            let edge_id_bytes = Self::uuid_to_bytes(&id);
            edges_table.remove(&edge_id_bytes)?;
        }

        write_txn.commit()?;
        self.decrement_meta_counter(STATS_EDGE_COUNT_KEY)?;
        Ok(())
    }

    fn edges_from(&self, node_id: NodeId) -> Result<Vec<Edge>> {
        let read_txn = self.db.begin_read()?;
        let edges_table = read_txn.open_table(EDGES)?;
        let from_index = read_txn.open_multimap_table(EDGES_BY_FROM)?;

        let node_id_bytes = Self::uuid_to_bytes(&node_id);
        let edge_ids: Vec<EdgeId> = from_index
            .get(&node_id_bytes)?
            .map(|result| result.map(|guard| Self::bytes_to_uuid(guard.value())))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut edges = Vec::new();
        for edge_id in edge_ids {
            let edge_id_bytes = Self::uuid_to_bytes(&edge_id);
            if let Some(bytes) = edges_table.get(&edge_id_bytes)? {
                edges.push(Self::deserialize_edge(bytes.value())?);
            }
        }

        Ok(edges)
    }

    fn edges_to(&self, node_id: NodeId) -> Result<Vec<Edge>> {
        let read_txn = self.db.begin_read()?;
        let edges_table = read_txn.open_table(EDGES)?;
        let to_index = read_txn.open_multimap_table(EDGES_BY_TO)?;

        let node_id_bytes = Self::uuid_to_bytes(&node_id);
        let edge_ids: Vec<EdgeId> = to_index
            .get(&node_id_bytes)?
            .map(|result| result.map(|guard| Self::bytes_to_uuid(guard.value())))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut edges = Vec::new();
        for edge_id in edge_ids {
            let edge_id_bytes = Self::uuid_to_bytes(&edge_id);
            if let Some(bytes) = edges_table.get(&edge_id_bytes)? {
                edges.push(Self::deserialize_edge(bytes.value())?);
            }
        }

        Ok(edges)
    }

    fn edges_between(&self, from: NodeId, to: NodeId) -> Result<Vec<Edge>> {
        let edges_from_node = self.edges_from(from)?;
        Ok(edges_from_node
            .into_iter()
            .filter(|e| e.to == to)
            .collect())
    }

    fn put_nodes_batch(&self, nodes: &[Node]) -> Result<()> {
        // Validate all nodes first
        for node in nodes {
            node.validate()
                .map_err(|e| CortexError::Validation(e))?;
        }

        let write_txn = self.db.begin_write()?;

        for node in nodes {
            // Get old node if exists
            let node_id_bytes = Self::uuid_to_bytes(&node.id);
            let old_node = {
                let nodes_table = write_txn.open_table(NODES)?;
                let old_bytes = nodes_table.get(&node_id_bytes)?.map(|guard| guard.value().to_vec());
                old_bytes.map(|bytes| Self::deserialize_node(&bytes)).transpose()?
            };

            // Serialize and store
            let node_bytes = Self::serialize_node(node)?;
            {
                let mut nodes_table = write_txn.open_table(NODES)?;
                let node_id_bytes = Self::uuid_to_bytes(&node.id);
                nodes_table.insert(&node_id_bytes, node_bytes.as_slice())?;
            }

            // Update indexes
            self.update_node_indexes(&write_txn, node, old_node.as_ref())?;
        }

        write_txn.commit()?;
        Ok(())
    }

    fn put_edges_batch(&self, edges: &[Edge]) -> Result<()> {
        // Validate all edges first
        for edge in edges {
            edge.validate()
                .map_err(|e| CortexError::Validation(e))?;
        }

        let write_txn = self.db.begin_write()?;

        for edge in edges {
            let edge_bytes = Self::serialize_edge(edge)?;
            {
                let mut edges_table = write_txn.open_table(EDGES)?;
                let edge_id_bytes = Self::uuid_to_bytes(&edge.id);
                edges_table.insert(&edge_id_bytes, edge_bytes.as_slice())?;
            }

            self.update_edge_indexes(&write_txn, edge)?;
        }

        write_txn.commit()?;
        Ok(())
    }

    fn put_metadata(&self, key: &str, value: &[u8]) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut meta_table = write_txn.open_table(META)?;
            meta_table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_metadata(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let read_txn = self.db.begin_read()?;
        let meta_table = read_txn.open_table(META)?;

        match meta_table.get(key)? {
            Some(value) => Ok(Some(value.value().to_vec())),
            None => Ok(None),
        }
    }

    fn compact(&self) -> Result<()> {
        // redb handles compaction automatically
        // This is a no-op but kept for API compatibility
        Ok(())
    }

    fn stats(&self) -> Result<StorageStats> {
        // Read O(1) counters from META for total counts
        let node_count = self.read_meta_counter(STATS_NODE_COUNT_KEY)?.unwrap_or_else(|| {
            // Legacy fallback: count from table scan
            self.db.begin_read().ok()
                .and_then(|txn| txn.open_table(NODES).ok())
                .and_then(|t| t.iter().ok().map(|it| it.count() as u64))
                .unwrap_or(0)
        });
        let edge_count = self.read_meta_counter(STATS_EDGE_COUNT_KEY)?.unwrap_or_else(|| {
            self.db.begin_read().ok()
                .and_then(|txn| txn.open_table(EDGES).ok())
                .and_then(|t| t.iter().ok().map(|it| it.count() as u64))
                .unwrap_or(0)
        });

        // Still scan for per-kind/per-relation breakdowns and timestamps
        let read_txn = self.db.begin_read()?;
        let nodes_table = read_txn.open_table(NODES)?;
        let edges_table = read_txn.open_table(EDGES)?;

        let mut node_counts_by_kind = HashMap::new();
        let mut edge_counts_by_relation = HashMap::new();
        let mut oldest_node: Option<DateTime<Utc>> = None;
        let mut newest_node: Option<DateTime<Utc>> = None;

        for item in nodes_table.iter()? {
            let (_, value) = item?;
            let node: Node = Self::deserialize_node(value.value())?;
            if !node.deleted {
                *node_counts_by_kind.entry(node.kind).or_insert(0) += 1;
                if oldest_node.is_none() || node.created_at < oldest_node.unwrap() {
                    oldest_node = Some(node.created_at);
                }
                if newest_node.is_none() || node.created_at > newest_node.unwrap() {
                    newest_node = Some(node.created_at);
                }
            }
        }

        for item in edges_table.iter()? {
            let (_, value) = item?;
            let edge: Edge = Self::deserialize_edge(value.value())?;
            *edge_counts_by_relation.entry(edge.relation).or_insert(0) += 1;
        }

        let db_size_bytes = std::fs::metadata(&self.path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(StorageStats {
            node_count,
            edge_count,
            node_counts_by_kind,
            edge_counts_by_relation,
            db_size_bytes,
            oldest_node,
            newest_node,
        })
    }

    fn snapshot(&self, path: &Path) -> Result<()> {
        std::fs::copy(&self.path, path).map_err(|e| {
            CortexError::Validation(format!("Failed to create snapshot: {}", e))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn create_test_storage() -> (RedbStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.redb");
        let storage = RedbStorage::open(&db_path).unwrap();
        (storage, temp_dir)
    }

    fn create_test_node(kind: NodeKind, title: &str) -> Node {
        Node::new(
            kind,
            title.to_string(),
            "Test body".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    #[test]
    fn test_node_crud() {
        let (storage, _temp) = create_test_storage();

        let node = create_test_node(NodeKind::new("fact").unwrap(), "Test Fact");

        // Create
        storage.put_node(&node).unwrap();

        // Read
        let retrieved = storage.get_node(node.id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().data.title, "Test Fact");

        // Update
        let mut updated = node.clone();
        updated.data.title = "Updated Fact".to_string();
        storage.put_node(&updated).unwrap();

        let retrieved = storage.get_node(node.id).unwrap().unwrap();
        assert_eq!(retrieved.data.title, "Updated Fact");

        // Soft delete
        storage.delete_node(node.id).unwrap();
        let deleted = storage.get_node(node.id).unwrap().unwrap();
        assert!(deleted.deleted);
    }

    #[test]
    fn test_node_validation() {
        let (storage, _temp) = create_test_storage();

        // Title too long
        let mut node = create_test_node(NodeKind::new("fact").unwrap(), &"x".repeat(300));
        let result = storage.put_node(&node);
        assert!(result.is_err());

        // Invalid tag
        node.data.title = "Valid".to_string();
        node.data.tags = vec!["Invalid Tag!".to_string()];
        let result = storage.put_node(&node);
        assert!(result.is_err());

        // Valid node
        node.data.tags = vec!["valid-tag".to_string()];
        let result = storage.put_node(&node);
        assert!(result.is_ok());
    }

    #[test]
    fn test_edge_crud() {
        let (storage, _temp) = create_test_storage();

        // Create two nodes
        let node1 = create_test_node(NodeKind::new("fact").unwrap(), "Fact 1");
        let node2 = create_test_node(NodeKind::new("decision").unwrap(), "Decision 1");
        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();

        // Create edge
        let edge = Edge::new(
            node1.id,
            node2.id,
            Relation::new("informed_by").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        // Read
        let retrieved = storage.get_edge(edge.id).unwrap();
        assert!(retrieved.is_some());

        // Delete
        storage.delete_edge(edge.id).unwrap();
        let deleted = storage.get_edge(edge.id).unwrap();
        assert!(deleted.is_none());
    }

    #[test]
    fn test_edge_validation() {
        let (storage, _temp) = create_test_storage();

        let node = create_test_node(NodeKind::new("fact").unwrap(), "Fact");
        storage.put_node(&node).unwrap();

        // Self-edge
        let edge = Edge::new(
            node.id,
            node.id,
            Relation::new("related_to").unwrap(),
            0.5,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        assert!(storage.put_edge(&edge).is_err());

        // Non-existent target node
        let edge = Edge::new(
            node.id,
            Uuid::now_v7(),
            Relation::new("related_to").unwrap(),
            0.5,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        assert!(storage.put_edge(&edge).is_err());
    }

    #[test]
    fn test_node_filtering() {
        let (storage, _temp) = create_test_storage();

        // Create nodes of different kinds
        let fact = create_test_node(NodeKind::new("fact").unwrap(), "Fact");
        let decision = create_test_node(NodeKind::new("decision").unwrap(), "Decision");
        storage.put_node(&fact).unwrap();
        storage.put_node(&decision).unwrap();

        // Filter by kind
        let filter = NodeFilter::new().with_kinds(vec![NodeKind::new("fact").unwrap()]);
        let results = storage.list_nodes(filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, NodeKind::new("fact").unwrap());

        // Filter by multiple kinds
        let filter = NodeFilter::new().with_kinds(vec![NodeKind::new("fact").unwrap(), NodeKind::new("decision").unwrap()]);
        let results = storage.list_nodes(filter).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_edge_traversal() {
        let (storage, _temp) = create_test_storage();

        let node1 = create_test_node(NodeKind::new("fact").unwrap(), "Node 1");
        let node2 = create_test_node(NodeKind::new("fact").unwrap(), "Node 2");
        let node3 = create_test_node(NodeKind::new("fact").unwrap(), "Node 3");

        storage.put_node(&node1).unwrap();
        storage.put_node(&node2).unwrap();
        storage.put_node(&node3).unwrap();

        // Create edges: 1->2, 1->3, 3->2
        let edge1 = Edge::new(
            node1.id,
            node2.id,
            Relation::new("related_to").unwrap(),
            1.0,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        let edge2 = Edge::new(
            node1.id,
            node3.id,
            Relation::new("led_to").unwrap(),
            1.0,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        let edge3 = Edge::new(
            node3.id,
            node2.id,
            Relation::new("informed_by").unwrap(),
            1.0,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );

        storage.put_edge(&edge1).unwrap();
        storage.put_edge(&edge2).unwrap();
        storage.put_edge(&edge3).unwrap();

        // Test edges_from
        let from_node1 = storage.edges_from(node1.id).unwrap();
        assert_eq!(from_node1.len(), 2);

        // Test edges_to
        let to_node2 = storage.edges_to(node2.id).unwrap();
        assert_eq!(to_node2.len(), 2);

        // Test edges_between
        let between = storage.edges_between(node1.id, node2.id).unwrap();
        assert_eq!(between.len(), 1);
    }

    #[test]
    fn test_batch_operations() {
        let (storage, _temp) = create_test_storage();

        let nodes: Vec<Node> = (0..10)
            .map(|i| create_test_node(NodeKind::new("observation").unwrap(), &format!("Node {}", i)))
            .collect();

        storage.put_nodes_batch(&nodes).unwrap();

        let filter = NodeFilter::new();
        let results = storage.list_nodes(filter).unwrap();
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn test_storage_stats() {
        let (storage, _temp) = create_test_storage();

        let fact = create_test_node(NodeKind::new("fact").unwrap(), "Fact");
        let decision = create_test_node(NodeKind::new("decision").unwrap(), "Decision");
        storage.put_node(&fact).unwrap();
        storage.put_node(&decision).unwrap();

        let edge = Edge::new(
            fact.id,
            decision.id,
            Relation::new("informed_by").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        let stats = storage.stats().unwrap();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.node_counts_by_kind.get(&NodeKind::new("fact").unwrap()), Some(&1));
    }
}

#[cfg(test)]
mod optimization_tests {
    use super::*;
    use crate::types::*;
    use tempfile::TempDir;

    fn create_test_storage() -> (RedbStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("opt_test.redb");
        let storage = RedbStorage::open(&db_path).unwrap();
        (storage, temp_dir)
    }

    fn make_node(kind: NodeKind, title: &str) -> Node {
        Node::new(kind, title.to_string(), "body".to_string(),
            Source { agent: "test".to_string(), session: None, channel: None }, 0.5)
    }

    #[test]
    fn test_count_nodes_optimized_path() {
        let (storage, _temp) = create_test_storage();

        for i in 0..50 {
            storage.put_node(&make_node(NodeKind::new("fact").unwrap(), &format!("F{}", i))).unwrap();
        }
        for i in 0..30 {
            storage.put_node(&make_node(NodeKind::new("decision").unwrap(), &format!("D{}", i))).unwrap();
        }

        // Kind-only filter should use index fast path
        let count = storage.count_nodes(NodeFilter::new().with_kinds(vec![NodeKind::new("fact").unwrap()])).unwrap();
        assert_eq!(count, 50);

        let count = storage.count_nodes(NodeFilter::new().with_kinds(vec![NodeKind::new("fact").unwrap(), NodeKind::new("decision").unwrap()])).unwrap();
        assert_eq!(count, 80);

        let count = storage.count_nodes(NodeFilter::new()).unwrap();
        assert_eq!(count, 80);
    }

    #[test]
    fn test_duplicate_edge_detection() {
        let (storage, _temp) = create_test_storage();

        let n1 = make_node(NodeKind::new("fact").unwrap(), "N1");
        let n2 = make_node(NodeKind::new("fact").unwrap(), "N2");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();

        let e1 = Edge::new(n1.id, n2.id, Relation::new("related_to").unwrap(), 0.8,
            EdgeProvenance::Manual { created_by: "test".to_string() });
        storage.put_edge(&e1).unwrap();

        // Same from/to/relation should fail
        let e2 = Edge::new(n1.id, n2.id, Relation::new("related_to").unwrap(), 0.5,
            EdgeProvenance::Manual { created_by: "test".to_string() });
        assert!(storage.put_edge(&e2).is_err());

        // Different relation should succeed
        let e3 = Edge::new(n1.id, n2.id, Relation::new("led_to").unwrap(), 0.5,
            EdgeProvenance::Manual { created_by: "test".to_string() });
        assert!(storage.put_edge(&e3).is_ok());
    }

    #[test]
    fn test_edge_to_deleted_node_rejected() {
        let (storage, _temp) = create_test_storage();

        let n1 = make_node(NodeKind::new("fact").unwrap(), "N1");
        let n2 = make_node(NodeKind::new("fact").unwrap(), "N2");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();
        storage.delete_node(n2.id).unwrap();

        let edge = Edge::new(n1.id, n2.id, Relation::new("related_to").unwrap(), 0.8,
            EdgeProvenance::Manual { created_by: "test".to_string() });
        assert!(storage.put_edge(&edge).is_err());
    }

    #[test]
    fn test_update_existing_edge() {
        let (storage, _temp) = create_test_storage();

        let n1 = make_node(NodeKind::new("fact").unwrap(), "N1");
        let n2 = make_node(NodeKind::new("fact").unwrap(), "N2");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();

        let mut edge = Edge::new(n1.id, n2.id, Relation::new("related_to").unwrap(), 0.8,
            EdgeProvenance::Manual { created_by: "test".to_string() });
        storage.put_edge(&edge).unwrap();

        // Update same edge (same ID) should succeed
        edge.weight = 0.3;
        storage.put_edge(&edge).unwrap();

        let retrieved = storage.get_edge(edge.id).unwrap().unwrap();
        assert!((retrieved.weight - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_snapshot_and_restore() {
        let (storage, _temp) = create_test_storage();

        let node = make_node(NodeKind::new("fact").unwrap(), "Snapshot test");
        storage.put_node(&node).unwrap();

        // Snapshot
        let snapshot_path = _temp.path().join("snapshot.redb");
        storage.snapshot(&snapshot_path).unwrap();

        // Open snapshot as separate database
        let restored = RedbStorage::open(&snapshot_path).unwrap();
        let retrieved = restored.get_node(node.id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().data.title, "Snapshot test");
    }

    #[test]
    fn test_tag_index_update_on_node_change() {
        let (storage, _temp) = create_test_storage();

        let mut node = make_node(NodeKind::new("fact").unwrap(), "Tagged");
        node.data.tags = vec!["alpha".to_string(), "beta".to_string()];
        storage.put_node(&node).unwrap();

        // Find by tag
        let results = storage.list_nodes(NodeFilter::new().with_tags(vec!["alpha".to_string()])).unwrap();
        assert_eq!(results.len(), 1);

        // Update tags — remove alpha, add gamma
        node.data.tags = vec!["beta".to_string(), "gamma".to_string()];
        storage.put_node(&node).unwrap();

        // Alpha should no longer match
        let results = storage.list_nodes(NodeFilter::new().with_tags(vec!["alpha".to_string()])).unwrap();
        assert_eq!(results.len(), 0);

        // Gamma should match
        let results = storage.list_nodes(NodeFilter::new().with_tags(vec!["gamma".to_string()])).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_source_index_update_on_agent_change() {
        let (storage, _temp) = create_test_storage();

        let mut node = make_node(NodeKind::new("fact").unwrap(), "Agent test");
        storage.put_node(&node).unwrap();

        let results = storage.list_nodes(NodeFilter::new().with_source_agent("test".to_string())).unwrap();
        assert_eq!(results.len(), 1);

        // Change agent
        node.source.agent = "kai".to_string();
        storage.put_node(&node).unwrap();

        let results = storage.list_nodes(NodeFilter::new().with_source_agent("test".to_string())).unwrap();
        assert_eq!(results.len(), 0);

        let results = storage.list_nodes(NodeFilter::new().with_source_agent("kai".to_string())).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_deleted_nodes_excluded_by_default() {
        let (storage, _temp) = create_test_storage();

        let n1 = make_node(NodeKind::new("fact").unwrap(), "Alive");
        let n2 = make_node(NodeKind::new("fact").unwrap(), "Dead");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();
        storage.delete_node(n2.id).unwrap();

        let results = storage.list_nodes(NodeFilter::new()).unwrap();
        assert_eq!(results.len(), 1);

        let results = storage.list_nodes(NodeFilter::new().include_deleted()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_importance_filter() {
        let (storage, _temp) = create_test_storage();

        let mut low = make_node(NodeKind::new("fact").unwrap(), "Low importance");
        low.importance = 0.2;
        let mut high = make_node(NodeKind::new("fact").unwrap(), "High importance");
        high.importance = 0.9;

        storage.put_node(&low).unwrap();
        storage.put_node(&high).unwrap();

        let results = storage.list_nodes(NodeFilter::new().with_min_importance(0.5)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data.title, "High importance");
    }

    #[test]
    fn test_pagination() {
        let (storage, _temp) = create_test_storage();

        for i in 0..20 {
            storage.put_node(&make_node(NodeKind::new("fact").unwrap(), &format!("Page {}", i))).unwrap();
        }

        let page1 = storage.list_nodes(NodeFilter::new().with_limit(5)).unwrap();
        assert_eq!(page1.len(), 5);

        let page2 = storage.list_nodes(NodeFilter::new().with_limit(5).with_offset(5)).unwrap();
        assert_eq!(page2.len(), 5);

        // Pages shouldn't overlap
        let page1_ids: Vec<_> = page1.iter().map(|n| n.id).collect();
        let page2_ids: Vec<_> = page2.iter().map(|n| n.id).collect();
        assert!(page1_ids.iter().all(|id| !page2_ids.contains(id)));
    }

    #[test]
    fn test_concurrent_read_during_iteration() {
        let (storage, _temp) = create_test_storage();

        // Insert some nodes
        for i in 0..100 {
            storage.put_node(&make_node(NodeKind::new("fact").unwrap(), &format!("N{}", i))).unwrap();
        }

        // Stats should work (iterates all nodes)
        let stats = storage.stats().unwrap();
        assert_eq!(stats.node_count, 100);
    }
}
