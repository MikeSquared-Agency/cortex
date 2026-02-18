use crate::error::Result;
use crate::graph::{Path, PathRequest, PathResult};
use crate::storage::Storage;
use crate::types::{EdgeId, NodeId};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

/// State for Dijkstra's algorithm
#[derive(Clone, Debug)]
struct DijkstraState {
    node: NodeId,
    cost: f32,
    path: Vec<NodeId>,
    edges: Vec<EdgeId>,
}

impl PartialEq for DijkstraState {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for DijkstraState {}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Lower cost = higher priority (min-heap)
        other.cost.partial_cmp(&self.cost)
    }
}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

/// Find paths between two nodes
pub fn find_paths<S: Storage>(storage: &S, request: PathRequest) -> Result<PathResult> {
    if request.max_paths == 1 {
        // Single shortest path
        if request.min_weight.is_some() {
            find_weighted_shortest_path(storage, &request)
        } else {
            find_unweighted_shortest_path(storage, &request)
        }
    } else {
        // K-shortest paths using Yen's algorithm
        find_k_shortest_paths(storage, &request)
    }
}

/// Find unweighted shortest path using BFS
fn find_unweighted_shortest_path<S: Storage>(
    storage: &S,
    request: &PathRequest,
) -> Result<PathResult> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut parent: HashMap<NodeId, (NodeId, EdgeId)> = HashMap::new();

    queue.push_back(request.from);
    visited.insert(request.from);

    while let Some(current) = queue.pop_front() {
        if current == request.to {
            // Found path, reconstruct it
            let path = reconstruct_path(request.from, request.to, &parent, storage)?;
            return Ok(PathResult {
                paths: vec![path],
            });
        }

        // Check max length
        let current_depth = calculate_depth(request.from, current, &parent);
        if let Some(max_len) = request.max_length {
            if current_depth >= max_len {
                continue;
            }
        }

        // Get outgoing edges
        let edges = storage.edges_from(current)?;

        for edge in edges {
            // Apply filters
            if let Some(ref relations) = request.relation_filter {
                if !relations.contains(&edge.relation) {
                    continue;
                }
            }

            if let Some(min_weight) = request.min_weight {
                if edge.weight < min_weight {
                    continue;
                }
            }

            if !visited.contains(&edge.to) {
                visited.insert(edge.to);
                parent.insert(edge.to, (current, edge.id));
                queue.push_back(edge.to);
            }
        }
    }

    // No path found
    Ok(PathResult { paths: vec![] })
}

/// Find weighted shortest path using Dijkstra (higher weight = lower cost)
fn find_weighted_shortest_path<S: Storage>(
    storage: &S,
    request: &PathRequest,
) -> Result<PathResult> {
    let mut heap = BinaryHeap::new();
    let mut visited = HashSet::new();

    heap.push(DijkstraState {
        node: request.from,
        cost: 0.0,
        path: vec![request.from],
        edges: vec![],
    });

    while let Some(DijkstraState {
        node,
        cost,
        path,
        edges: edge_path,
    }) = heap.pop()
    {
        if node == request.to {
            // Found path
            let total_weight = if !edge_path.is_empty() {
                calculate_path_weight(storage, &edge_path)?
            } else {
                1.0
            };

            return Ok(PathResult {
                paths: vec![Path::new(path, edge_path, total_weight)],
            });
        }

        if visited.contains(&node) {
            continue;
        }
        visited.insert(node);

        // Check max length
        if let Some(max_len) = request.max_length {
            if edge_path.len() >= max_len as usize {
                continue;
            }
        }

        // Get outgoing edges
        let edges = storage.edges_from(node)?;

        for edge in edges {
            // Apply filters
            if let Some(ref relations) = request.relation_filter {
                if !relations.contains(&edge.relation) {
                    continue;
                }
            }

            if let Some(min_weight) = request.min_weight {
                if edge.weight < min_weight {
                    continue;
                }
            }

            if !visited.contains(&edge.to) {
                // Higher weight = lower cost (invert for Dijkstra)
                let new_cost = cost + (1.0 - edge.weight);

                let mut new_path = path.clone();
                new_path.push(edge.to);

                let mut new_edges = edge_path.clone();
                new_edges.push(edge.id);

                heap.push(DijkstraState {
                    node: edge.to,
                    cost: new_cost,
                    path: new_path,
                    edges: new_edges,
                });
            }
        }
    }

    // No path found
    Ok(PathResult { paths: vec![] })
}

/// Find k-shortest paths using Yen's algorithm
fn find_k_shortest_paths<S: Storage>(
    storage: &S,
    request: &PathRequest,
) -> Result<PathResult> {
    let mut result_paths = Vec::new();

    // Find first shortest path
    let first_path_result = if request.min_weight.is_some() {
        find_weighted_shortest_path(storage, request)?
    } else {
        find_unweighted_shortest_path(storage, request)?
    };

    if first_path_result.paths.is_empty() {
        return Ok(PathResult { paths: vec![] });
    }

    result_paths.push(first_path_result.paths[0].clone());

    // Candidate paths
    let mut candidates: Vec<Path> = Vec::new();

    for k in 1..request.max_paths {
        if result_paths.len() < k {
            break;
        }

        let prev_path = &result_paths[k - 1];

        // For each node in the previous path (except the last)
        for i in 0..prev_path.nodes.len().saturating_sub(1) {
            let spur_node = prev_path.nodes[i];
            let root_path = &prev_path.nodes[..=i];

            // Create a modified request from spur node to destination
            // This is a simplified version - full Yen's algorithm would need
            // to remove edges/nodes used in previous paths
            let spur_request = PathRequest {
                from: spur_node,
                to: request.to,
                max_length: request.max_length.map(|l| l - i as u32),
                relation_filter: request.relation_filter.clone(),
                min_weight: request.min_weight,
                max_paths: 1,
            };

            let spur_result = if request.min_weight.is_some() {
                find_weighted_shortest_path(storage, &spur_request)?
            } else {
                find_unweighted_shortest_path(storage, &spur_request)?
            };

            if !spur_result.paths.is_empty() {
                let spur_path = &spur_result.paths[0];

                // Combine root path and spur path
                let mut total_nodes = root_path.to_vec();
                total_nodes.extend(&spur_path.nodes[1..]); // Skip spur node (already in root)

                let mut total_edges = prev_path.edges[..i].to_vec();
                total_edges.extend(&spur_path.edges);

                let total_weight = calculate_path_weight(storage, &total_edges)?;

                let candidate = Path::new(total_nodes, total_edges, total_weight);

                // Add to candidates if not already there
                if !candidates.iter().any(|p| p.nodes == candidate.nodes) {
                    candidates.push(candidate);
                }
            }
        }

        if candidates.is_empty() {
            break;
        }

        // Sort candidates by length/weight
        candidates.sort_by(|a, b| {
            a.length
                .cmp(&b.length)
                .then(b.total_weight.partial_cmp(&a.total_weight).unwrap_or(Ordering::Equal))
        });

        // Take the best candidate
        if let Some(best) = candidates.first() {
            result_paths.push(best.clone());
            candidates.remove(0);
        }
    }

    Ok(PathResult {
        paths: result_paths,
    })
}

/// Reconstruct path from parent map
fn reconstruct_path<S: Storage>(
    start: NodeId,
    end: NodeId,
    parent: &HashMap<NodeId, (NodeId, EdgeId)>,
    storage: &S,
) -> Result<Path> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut current = end;

    nodes.push(current);

    while current != start {
        if let Some(&(prev, edge_id)) = parent.get(&current) {
            nodes.push(prev);
            edges.push(edge_id);
            current = prev;
        } else {
            break;
        }
    }

    nodes.reverse();
    edges.reverse();

    let total_weight = calculate_path_weight(storage, &edges)?;

    Ok(Path::new(nodes, edges, total_weight))
}

/// Calculate depth of a node in BFS traversal
fn calculate_depth(start: NodeId, node: NodeId, parent: &HashMap<NodeId, (NodeId, EdgeId)>) -> u32 {
    let mut depth = 0;
    let mut current = node;

    while current != start {
        if let Some(&(prev, _)) = parent.get(&current) {
            depth += 1;
            current = prev;
        } else {
            break;
        }
    }

    depth
}

/// Calculate total weight of a path (product of edge weights)
fn calculate_path_weight<S: Storage>(storage: &S, edge_ids: &[EdgeId]) -> Result<f32> {
    let mut weight = 1.0;

    for edge_id in edge_ids {
        if let Some(edge) = storage.get_edge(*edge_id)? {
            weight *= edge.weight;
        }
    }

    Ok(weight)
}
