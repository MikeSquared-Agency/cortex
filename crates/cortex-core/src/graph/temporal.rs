use crate::error::Result;
use crate::graph::Subgraph;
use crate::storage::{NodeFilter, Storage};
use crate::types::{Node, NodeId, NodeKind};
use chrono::{DateTime, Utc};

/// Temporal query support
pub trait TemporalQueries: Send + Sync {
    /// Nodes created or updated since a given timestamp.
    /// Used by auto-linker to find new nodes to process.
    fn changed_since(&self, since: DateTime<Utc>) -> Result<Vec<Node>>;

    /// Snapshot of a node's neighborhood at a point in time.
    /// Only includes nodes and edges that existed at `at`.
    fn neighborhood_at(&self, id: NodeId, depth: u32, at: DateTime<Utc>) -> Result<Subgraph>;

    /// Timeline: ordered list of nodes created within a time range.
    fn timeline(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        kind_filter: Option<Vec<NodeKind>>,
    ) -> Result<Vec<Node>>;
}

/// Implementation of temporal queries for any storage backend
pub struct TemporalQueriesImpl<S: Storage> {
    storage: S,
}

impl<S: Storage> TemporalQueriesImpl<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }
}

impl<S: Storage> TemporalQueries for TemporalQueriesImpl<S> {
    fn changed_since(&self, since: DateTime<Utc>) -> Result<Vec<Node>> {
        let filter = NodeFilter::new().created_after(since);
        let mut nodes = self.storage.list_nodes(filter)?;

        // Also check for nodes updated after the timestamp
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;
        for node in all_nodes {
            if node.updated_at > since && node.created_at <= since {
                nodes.push(node);
            }
        }

        // Sort by updated_at
        nodes.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));

        Ok(nodes)
    }

    fn neighborhood_at(&self, id: NodeId, depth: u32, at: DateTime<Utc>) -> Result<Subgraph> {
        let mut subgraph = Subgraph::new();

        // Get the starting node if it existed at the given time
        if let Some(node) = self.storage.get_node(id)? {
            if node.created_at <= at {
                subgraph.nodes.insert(id, node);
                subgraph.depths.insert(id, 0);
            } else {
                // Node didn't exist at the given time
                return Ok(subgraph);
            }
        } else {
            return Ok(subgraph);
        }

        // BFS traversal with temporal filtering
        let mut current_level = vec![id];

        for current_depth in 0..depth {
            let mut next_level = Vec::new();

            for node_id in current_level {
                // Get edges that existed at the given time
                let mut edges = self.storage.edges_from(node_id)?;
                edges.extend(self.storage.edges_to(node_id)?);

                for edge in edges {
                    // Only include edges that existed at the given time
                    if edge.created_at > at {
                        continue;
                    }

                    // Get the neighbor
                    let neighbor_id = if edge.from == node_id {
                        edge.to
                    } else {
                        edge.from
                    };

                    // Skip if already in subgraph
                    if subgraph.nodes.contains_key(&neighbor_id) {
                        continue;
                    }

                    // Get neighbor node
                    if let Some(neighbor) = self.storage.get_node(neighbor_id)? {
                        // Only include if it existed at the given time
                        if neighbor.created_at <= at {
                            subgraph.nodes.insert(neighbor_id, neighbor);
                            subgraph.depths.insert(neighbor_id, current_depth + 1);
                            next_level.push(neighbor_id);

                            // Add edge to subgraph
                            subgraph.edges.push(edge);
                        }
                    }
                }
            }

            current_level = next_level;

            if current_level.is_empty() {
                break;
            }
        }

        Ok(subgraph)
    }

    fn timeline(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        kind_filter: Option<Vec<NodeKind>>,
    ) -> Result<Vec<Node>> {
        let mut filter = NodeFilter::new()
            .created_after(from)
            .created_before(to);

        if let Some(kinds) = kind_filter {
            filter = filter.with_kinds(kinds);
        }

        let mut nodes = self.storage.list_nodes(filter)?;

        // Sort by created_at
        nodes.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(nodes)
    }
}
