use crate::types::{Edge, EdgeId, Node, NodeId};
use std::collections::{HashMap, HashSet, VecDeque};

/// A subgraph result from traversal or query
#[derive(Debug, Clone)]
pub struct Subgraph {
    /// All nodes in the result, keyed by ID for O(1) lookup.
    pub nodes: HashMap<NodeId, Node>,

    /// All edges connecting the result nodes.
    pub edges: Vec<Edge>,

    /// Depth of each node from the nearest start node.
    pub depths: HashMap<NodeId, u32>,

    /// Total nodes visited during traversal (may be > nodes.len()
    /// if kind_filter excluded some).
    pub visited_count: usize,

    /// Whether traversal was truncated by limit.
    pub truncated: bool,
}

impl Subgraph {
    /// Create a new empty subgraph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            depths: HashMap::new(),
            visited_count: 0,
            truncated: false,
        }
    }

    /// Get all nodes at a specific depth
    pub fn at_depth(&self, depth: u32) -> Vec<&Node> {
        self.depths
            .iter()
            .filter(|(_, &d)| d == depth)
            .filter_map(|(id, _)| self.nodes.get(id))
            .collect()
    }

    /// Get all edges between two specific nodes
    pub fn edges_between(&self, a: NodeId, b: NodeId) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|e| (e.from == a && e.to == b) || (e.from == b && e.to == a))
            .collect()
    }

    /// Get neighbors of a node within this subgraph
    pub fn neighbors(&self, id: NodeId) -> Vec<&Node> {
        let mut neighbor_ids = HashSet::new();

        for edge in &self.edges {
            if edge.from == id {
                neighbor_ids.insert(edge.to);
            } else if edge.to == id {
                neighbor_ids.insert(edge.from);
            }
        }

        neighbor_ids
            .iter()
            .filter_map(|nid| self.nodes.get(nid))
            .collect()
    }

    /// Topological sort (if DAG). Returns None if cycles exist.
    pub fn topo_sort(&self) -> Option<Vec<NodeId>> {
        // Kahn's algorithm
        let mut in_degree: HashMap<NodeId, usize> = self.nodes.keys().map(|&id| (id, 0)).collect();

        // Calculate in-degrees
        for edge in &self.edges {
            if in_degree.contains_key(&edge.to) {
                *in_degree.get_mut(&edge.to).unwrap() += 1;
            }
        }

        // Start with nodes that have no incoming edges
        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            result.push(node_id);

            // Reduce in-degree for neighbors
            for edge in &self.edges {
                if edge.from == node_id {
                    if let Some(deg) = in_degree.get_mut(&edge.to) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(edge.to);
                        }
                    }
                }
            }
        }

        // If we processed all nodes, it's a DAG
        if result.len() == self.nodes.len() {
            Some(result)
        } else {
            None // Cycle detected
        }
    }

    /// Merge another subgraph into this one
    pub fn merge(&mut self, other: Subgraph) {
        // Merge nodes
        for (id, node) in other.nodes {
            self.nodes.insert(id, node);
        }

        // Merge edges (deduplicate by edge ID)
        let existing_edge_ids: HashSet<EdgeId> = self.edges.iter().map(|e| e.id).collect();
        for edge in other.edges {
            if !existing_edge_ids.contains(&edge.id) {
                self.edges.push(edge);
            }
        }

        // Merge depths (keep minimum depth for each node)
        for (id, depth) in other.depths {
            self.depths
                .entry(id)
                .and_modify(|d| *d = (*d).min(depth))
                .or_insert(depth);
        }

        // Update visited count
        self.visited_count += other.visited_count;

        // Update truncated flag
        self.truncated = self.truncated || other.truncated;
    }

    /// Get the number of nodes in the subgraph
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the subgraph
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Check if the subgraph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the maximum depth in the subgraph
    pub fn max_depth(&self) -> Option<u32> {
        self.depths.values().max().copied()
    }
}

impl Default for Subgraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EdgeProvenance, Node, NodeKind, Relation, Source};
    use uuid::Uuid;

    fn create_test_node(id: NodeId, title: &str) -> Node {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            title.to_string(),
            "Test".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );
        node.id = id;
        node
    }

    fn create_test_edge(from: NodeId, to: NodeId) -> Edge {
        Edge::new(
            from,
            to,
            Relation::new("related_to").unwrap(),
            1.0,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        )
    }

    #[test]
    fn test_at_depth() {
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let id3 = Uuid::now_v7();

        let mut subgraph = Subgraph::new();
        subgraph.nodes.insert(id1, create_test_node(id1, "Node 1"));
        subgraph.nodes.insert(id2, create_test_node(id2, "Node 2"));
        subgraph.nodes.insert(id3, create_test_node(id3, "Node 3"));

        subgraph.depths.insert(id1, 0);
        subgraph.depths.insert(id2, 1);
        subgraph.depths.insert(id3, 1);

        let at_depth_1 = subgraph.at_depth(1);
        assert_eq!(at_depth_1.len(), 2);
    }

    #[test]
    fn test_topo_sort_dag() {
        // Create a simple DAG: A -> B -> C
        let id_a = Uuid::now_v7();
        let id_b = Uuid::now_v7();
        let id_c = Uuid::now_v7();

        let mut subgraph = Subgraph::new();
        subgraph.nodes.insert(id_a, create_test_node(id_a, "A"));
        subgraph.nodes.insert(id_b, create_test_node(id_b, "B"));
        subgraph.nodes.insert(id_c, create_test_node(id_c, "C"));

        subgraph.edges.push(create_test_edge(id_a, id_b));
        subgraph.edges.push(create_test_edge(id_b, id_c));

        let sorted = subgraph.topo_sort();
        assert!(sorted.is_some());
        let sorted = sorted.unwrap();
        assert_eq!(sorted.len(), 3);

        // A should come before B, B before C
        let pos_a = sorted.iter().position(|&x| x == id_a).unwrap();
        let pos_b = sorted.iter().position(|&x| x == id_b).unwrap();
        let pos_c = sorted.iter().position(|&x| x == id_c).unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_topo_sort_cycle() {
        // Create a cycle: A -> B -> C -> A
        let id_a = Uuid::now_v7();
        let id_b = Uuid::now_v7();
        let id_c = Uuid::now_v7();

        let mut subgraph = Subgraph::new();
        subgraph.nodes.insert(id_a, create_test_node(id_a, "A"));
        subgraph.nodes.insert(id_b, create_test_node(id_b, "B"));
        subgraph.nodes.insert(id_c, create_test_node(id_c, "C"));

        subgraph.edges.push(create_test_edge(id_a, id_b));
        subgraph.edges.push(create_test_edge(id_b, id_c));
        subgraph.edges.push(create_test_edge(id_c, id_a));

        let sorted = subgraph.topo_sort();
        assert!(sorted.is_none()); // Should detect cycle
    }

    #[test]
    fn test_merge() {
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let mut subgraph1 = Subgraph::new();
        subgraph1.nodes.insert(id1, create_test_node(id1, "Node 1"));
        subgraph1.depths.insert(id1, 0);

        let mut subgraph2 = Subgraph::new();
        subgraph2.nodes.insert(id2, create_test_node(id2, "Node 2"));
        subgraph2.depths.insert(id2, 1);

        subgraph1.merge(subgraph2);

        assert_eq!(subgraph1.nodes.len(), 2);
        assert_eq!(subgraph1.depths.len(), 2);
    }
}
