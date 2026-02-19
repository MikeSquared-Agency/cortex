use crate::error::Result;
use crate::graph::{
    cache::AdjacencyCache,
    paths, traversal, PathRequest, PathResult, Subgraph, TraversalBudget, TraversalDirection,
    TraversalRequest, TraversalStrategy,
};
use crate::storage::{NodeFilter, Storage};
use crate::types::{Edge, Node, NodeId, Relation};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

/// Graph query and traversal engine
pub trait GraphEngine: Send + Sync {
    /// Core traversal. Everything else builds on this.
    fn traverse(&self, request: TraversalRequest) -> Result<Subgraph>;

    /// Find paths between two nodes.
    fn find_paths(&self, request: PathRequest) -> Result<PathResult>;

    // --- Convenience methods ---

    /// Direct neighbors of a node (depth 1).
    fn neighbors(
        &self,
        id: NodeId,
        direction: TraversalDirection,
        relation_filter: Option<Vec<Relation>>,
    ) -> Result<Vec<(Node, Edge)>>;

    /// Everything within N hops of a node.
    fn neighborhood(&self, id: NodeId, depth: u32) -> Result<Subgraph>;

    /// All nodes that a given node can reach (transitive closure).
    fn reachable(&self, id: NodeId, direction: TraversalDirection) -> Result<Vec<NodeId>>;

    /// Find all nodes with no incoming edges of a given relation.
    /// Useful for finding "root causes" or "original decisions."
    fn roots(&self, relation: Relation) -> Result<Vec<Node>>;

    /// Find all nodes with no outgoing edges of a given relation.
    /// Useful for finding "leaf outcomes" or "terminal states."
    fn leaves(&self, relation: Relation) -> Result<Vec<Node>>;

    /// Detect cycles in the graph (or within a subgraph).
    fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>>;

    /// Connected components. Groups of nodes that can reach each other.
    fn components(&self) -> Result<Vec<Vec<NodeId>>>;

    /// Degree centrality: which nodes have the most connections?
    /// Returns nodes sorted by total edge count (in + out).
    fn most_connected(&self, limit: usize) -> Result<Vec<(Node, usize)>>;
}

/// Implementation of the graph engine
pub struct GraphEngineImpl<S: Storage> {
    storage: Arc<S>,
    budget: TraversalBudget,
    cache: AdjacencyCache,
}

impl<S: Storage> GraphEngineImpl<S> {
    /// Create a new graph engine with the given storage
    pub fn new(storage: Arc<S>) -> Self {
        let cache = AdjacencyCache::new();
        Self {
            storage,
            budget: TraversalBudget::default(),
            cache,
        }
    }

    /// Create a new graph engine with custom budget
    pub fn with_budget(storage: Arc<S>, budget: TraversalBudget) -> Self {
        let cache = AdjacencyCache::new();
        Self { storage, budget, cache }
    }

    /// Ensure the adjacency cache is valid, rebuilding if necessary
    fn ensure_cache(&self) -> Result<()> {
        if !self.cache.is_valid() {
            self.cache.build(self.storage.as_ref())?;
        }
        Ok(())
    }

    /// Invalidate the cache (call after writes)
    pub fn invalidate_cache(&self) {
        self.cache.invalidate();
    }

    /// Get edges using cache when available, falling back to storage
    pub fn cached_edges_from(&self, node_id: NodeId) -> Result<Vec<Edge>> {
        if let Some(entries) = self.cache.get_outgoing(node_id) {
            // Reconstruct edges from cache entries
            let mut edges = Vec::with_capacity(entries.len());
            for entry in entries {
                if let Some(edge) = self.storage.get_edge(entry.edge_id)? {
                    edges.push(edge);
                }
            }
            Ok(edges)
        } else {
            self.storage.edges_from(node_id)
        }
    }

    /// Get edges using cache when available, falling back to storage
    pub fn cached_edges_to(&self, node_id: NodeId) -> Result<Vec<Edge>> {
        if let Some(entries) = self.cache.get_incoming(node_id) {
            let mut edges = Vec::with_capacity(entries.len());
            for entry in entries {
                if let Some(edge) = self.storage.get_edge(entry.edge_id)? {
                    edges.push(edge);
                }
            }
            Ok(edges)
        } else {
            self.storage.edges_to(node_id)
        }
    }
}

impl<S: Storage + 'static> GraphEngine for GraphEngineImpl<S> {
    fn traverse(&self, request: TraversalRequest) -> Result<Subgraph> {
        traversal::traverse(self.storage.as_ref(), request, &self.budget)
    }

    fn find_paths(&self, request: PathRequest) -> Result<PathResult> {
        paths::find_paths(self.storage.as_ref(), request)
    }

    fn neighbors(
        &self,
        id: NodeId,
        direction: TraversalDirection,
        relation_filter: Option<Vec<Relation>>,
    ) -> Result<Vec<(Node, Edge)>> {
        self.ensure_cache()?;
        let edges = match direction {
            TraversalDirection::Outgoing => self.cached_edges_from(id)?,
            TraversalDirection::Incoming => self.cached_edges_to(id)?,
            TraversalDirection::Both => {
                let mut edges = self.cached_edges_from(id)?;
                edges.extend(self.cached_edges_to(id)?);
                edges
            }
        };

        let mut result = Vec::new();

        for edge in edges {
            // Apply relation filter
            if let Some(ref relations) = relation_filter {
                if !relations.contains(&edge.relation) {
                    continue;
                }
            }

            // Get the neighbor node
            let neighbor_id = if edge.from == id { edge.to } else { edge.from };

            if let Some(neighbor) = self.storage.get_node(neighbor_id)? {
                result.push((neighbor, edge));
            }
        }

        Ok(result)
    }

    fn neighborhood(&self, id: NodeId, depth: u32) -> Result<Subgraph> {
        self.traverse(TraversalRequest {
            start: vec![id],
            max_depth: Some(depth),
            direction: TraversalDirection::Both,
            relation_filter: None,
            kind_filter: None,
            min_weight: None,
            limit: None,
            strategy: TraversalStrategy::Bfs,
            include_start: true,
            created_after: None,
        })
    }

    fn reachable(&self, id: NodeId, direction: TraversalDirection) -> Result<Vec<NodeId>> {
        let subgraph = self.traverse(TraversalRequest {
            start: vec![id],
            max_depth: None,
            direction,
            relation_filter: None,
            kind_filter: None,
            min_weight: None,
            limit: None,
            strategy: TraversalStrategy::Bfs,
            include_start: false,
            created_after: None,
        })?;

        Ok(subgraph.nodes.keys().copied().collect())
    }

    fn roots(&self, relation: Relation) -> Result<Vec<Node>> {
        self.ensure_cache()?;
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;
        let mut roots = Vec::new();

        for node in all_nodes {
            if node.deleted {
                continue;
            }

            let incoming = self.cached_edges_to(node.id)?;
            let has_incoming = incoming.iter().any(|e| e.relation == relation);

            if !has_incoming {
                let outgoing = self.cached_edges_from(node.id)?;
                let has_outgoing = outgoing.iter().any(|e| e.relation == relation);
                if has_outgoing {
                    roots.push(node);
                }
            }
        }

        Ok(roots)
    }

    fn leaves(&self, relation: Relation) -> Result<Vec<Node>> {
        self.ensure_cache()?;
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;
        let mut leaves = Vec::new();

        for node in all_nodes {
            if node.deleted {
                continue;
            }

            let outgoing = self.cached_edges_from(node.id)?;
            let has_outgoing = outgoing.iter().any(|e| e.relation == relation);

            if !has_outgoing {
                let incoming = self.cached_edges_to(node.id)?;
                let has_incoming = incoming.iter().any(|e| e.relation == relation);
                if has_incoming {
                    leaves.push(node);
                }
            }
        }

        Ok(leaves)
    }

    fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        self.ensure_cache()?;
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;

        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut cycles = Vec::new();

        for node in &all_nodes {
            if node.deleted {
                continue;
            }
            if !visited.contains(&node.id) {
                self.find_cycles_dfs(
                    node.id,
                    &mut visited,
                    &mut rec_stack,
                    &mut Vec::new(),
                    &mut cycles,
                )?;
            }
        }

        Ok(cycles)
    }

    fn components(&self) -> Result<Vec<Vec<NodeId>>> {
        self.ensure_cache()?;
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;

        let mut visited = HashSet::new();
        let mut components = Vec::new();

        for node in &all_nodes {
            if node.deleted {
                continue;
            }
            if !visited.contains(&node.id) {
                let mut component = Vec::new();
                self.component_bfs(node.id, &mut visited, &mut component)?;
                components.push(component);
            }
        }

        Ok(components)
    }

    fn most_connected(&self, limit: usize) -> Result<Vec<(Node, usize)>> {
        self.ensure_cache()?;
        let all_nodes = self.storage.list_nodes(NodeFilter::new())?;

        let mut node_degrees = Vec::new();

        for node in all_nodes {
            if node.deleted {
                continue;
            }

            let outgoing = self.cached_edges_from(node.id)?;
            let incoming = self.cached_edges_to(node.id)?;
            let degree = outgoing.len() + incoming.len();

            node_degrees.push((node, degree));
        }

        // Sort by degree descending
        node_degrees.sort_by(|a, b| b.1.cmp(&a.1));

        // Take top N
        Ok(node_degrees.into_iter().take(limit).collect())
    }
}

/// Blanket impl: Arc<G> forwards all GraphEngine calls to G.
/// Allows using Arc<GraphEngineImpl<S>> with HybridSearch directly.
impl<G: GraphEngine> GraphEngine for std::sync::Arc<G> {
    fn traverse(&self, request: TraversalRequest) -> Result<Subgraph> {
        (**self).traverse(request)
    }
    fn find_paths(&self, request: PathRequest) -> Result<PathResult> {
        (**self).find_paths(request)
    }
    fn neighbors(
        &self,
        id: NodeId,
        direction: TraversalDirection,
        relation_filter: Option<Vec<Relation>>,
    ) -> Result<Vec<(Node, Edge)>> {
        (**self).neighbors(id, direction, relation_filter)
    }
    fn neighborhood(&self, id: NodeId, depth: u32) -> Result<Subgraph> {
        (**self).neighborhood(id, depth)
    }
    fn reachable(&self, id: NodeId, direction: TraversalDirection) -> Result<Vec<NodeId>> {
        (**self).reachable(id, direction)
    }
    fn roots(&self, relation: Relation) -> Result<Vec<Node>> {
        (**self).roots(relation)
    }
    fn leaves(&self, relation: Relation) -> Result<Vec<Node>> {
        (**self).leaves(relation)
    }
    fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        (**self).find_cycles()
    }
    fn components(&self) -> Result<Vec<Vec<NodeId>>> {
        (**self).components()
    }
    fn most_connected(&self, limit: usize) -> Result<Vec<(Node, usize)>> {
        (**self).most_connected(limit)
    }
}

impl<S: Storage> GraphEngineImpl<S> {
    /// Helper for cycle detection using DFS
    fn find_cycles_dfs(
        &self,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        rec_stack: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
        cycles: &mut Vec<Vec<NodeId>>,
    ) -> Result<()> {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        let outgoing = self.cached_edges_from(node)?;

        for edge in outgoing {
            if !visited.contains(&edge.to) {
                self.find_cycles_dfs(edge.to, visited, rec_stack, path, cycles)?;
            } else if rec_stack.contains(&edge.to) {
                // Found a cycle
                if let Some(pos) = path.iter().position(|&x| x == edge.to) {
                    let cycle = path[pos..].to_vec();
                    cycles.push(cycle);
                }
            }
        }

        path.pop();
        rec_stack.remove(&node);

        Ok(())
    }

    /// Helper for connected components using BFS
    fn component_bfs(
        &self,
        start: NodeId,
        visited: &mut HashSet<NodeId>,
        component: &mut Vec<NodeId>,
    ) -> Result<()> {
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited.insert(start);

        while let Some(node) = queue.pop_front() {
            component.push(node);

            // Get all edges (both directions)
            let mut edges = self.cached_edges_from(node)?;
            edges.extend(self.cached_edges_to(node)?);

            for edge in edges {
                let neighbor = if edge.from == node { edge.to } else { edge.from };

                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }

        Ok(())
    }
}
