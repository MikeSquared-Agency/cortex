use crate::error::Result;
use crate::graph::{Subgraph, TraversalBudget, TraversalDirection, TraversalRequest, TraversalStrategy};
use crate::storage::Storage;
use crate::types::{Edge, NodeId};
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::cmp::Ordering;
use std::time::Instant;

/// Weighted node for priority queue traversal
#[derive(Debug, Clone)]
struct WeightedNode {
    id: NodeId,
    depth: u32,
    weight: f32,
}

impl PartialEq for WeightedNode {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for WeightedNode {}

impl PartialOrd for WeightedNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Higher weight = higher priority
        other.weight.partial_cmp(&self.weight)
    }
}

impl Ord for WeightedNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

/// Perform graph traversal according to the request
pub fn traverse<S: Storage>(
    storage: &S,
    request: TraversalRequest,
    budget: &TraversalBudget,
) -> Result<Subgraph> {
    match request.strategy {
        TraversalStrategy::Bfs => traverse_bfs(storage, request, budget),
        TraversalStrategy::Dfs => traverse_dfs(storage, request, budget),
        TraversalStrategy::Weighted => traverse_weighted(storage, request, budget),
    }
}

/// Breadth-first traversal
fn traverse_bfs<S: Storage>(
    storage: &S,
    request: TraversalRequest,
    budget: &TraversalBudget,
) -> Result<Subgraph> {
    let start_time = Instant::now();
    let mut result = Subgraph::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut candidate_edges = Vec::new();

    // Initialize with start nodes
    for node_id in &request.start {
        queue.push_back((*node_id, 0u32));
        visited.insert(*node_id);
    }

    while let Some((current_id, depth)) = queue.pop_front() {
        // Check budget
        if result.visited_count >= budget.max_visited {
            result.truncated = true;
            break;
        }
        if start_time.elapsed().as_millis() > budget.max_time_ms as u128 {
            result.truncated = true;
            break;
        }

        result.visited_count += 1;

        // Get current node
        let current_node = match storage.get_node(current_id)? {
            Some(node) => node,
            None => continue,
        };

        // Check temporal filter
        if let Some(after) = request.created_after {
            if current_node.created_at < after {
                continue;
            }
        }

        // Add node if it matches kind filter and we're including it
        let should_include = if depth == 0 && !request.include_start {
            false
        } else {
            match &request.kind_filter {
                Some(kinds) => kinds.contains(&current_node.kind),
                None => true,
            }
        };

        if should_include {
            result.nodes.insert(current_id, current_node.clone());
            result.depths.insert(current_id, depth);

            // Check result limit
            if let Some(limit) = request.limit {
                if result.nodes.len() >= limit {
                    result.truncated = true;
                    break;
                }
            }
        }

        // Check depth limit before expanding
        if let Some(max_depth) = request.max_depth {
            if depth >= max_depth {
                continue;
            }
        }

        // Get edges based on direction
        let edges = get_edges_for_node(storage, current_id, &request.direction)?;

        // Circuit breaker: check nodes at this level
        let nodes_at_level: Vec<_> = queue.iter().filter(|(_, d)| *d == depth + 1).collect();
        if nodes_at_level.len() >= budget.max_nodes_per_level {
            result.truncated = true;
            break;
        }

        // Traverse edges
        for edge in edges {
            // Check relation filter
            if let Some(ref relations) = request.relation_filter {
                if !relations.contains(&edge.relation) {
                    continue;
                }
            }

            // Check weight filter
            if let Some(min_weight) = request.min_weight {
                if edge.weight < min_weight {
                    continue;
                }
            }

            // Check temporal filter for edges
            if let Some(after) = request.created_after {
                if edge.created_at < after {
                    continue;
                }
            }

            // Determine next node
            let next_id = if edge.from == current_id {
                edge.to
            } else {
                edge.from
            };

            // Visit if not already visited
            if !visited.contains(&next_id) {
                visited.insert(next_id);
                queue.push_back((next_id, depth + 1));
            }

            // Add edge if both ends are in result or will be
            candidate_edges.push(edge);
        }
    }

    // Post-pass: only include edges where both endpoints are in the result
    result.edges = candidate_edges
        .into_iter()
        .filter(|e| result.nodes.contains_key(&e.from) && result.nodes.contains_key(&e.to))
        .collect();

    Ok(result)
}

/// Depth-first traversal
fn traverse_dfs<S: Storage>(
    storage: &S,
    request: TraversalRequest,
    budget: &TraversalBudget,
) -> Result<Subgraph> {
    let start_time = Instant::now();
    let mut result = Subgraph::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let mut candidate_edges = Vec::new();

    // Initialize with start nodes
    for node_id in request.start.iter().rev() {
        stack.push((*node_id, 0u32));
        visited.insert(*node_id);
    }

    while let Some((current_id, depth)) = stack.pop() {
        // Check budget
        if result.visited_count >= budget.max_visited {
            result.truncated = true;
            break;
        }
        if start_time.elapsed().as_millis() > budget.max_time_ms as u128 {
            result.truncated = true;
            break;
        }

        result.visited_count += 1;

        // Get current node
        let current_node = match storage.get_node(current_id)? {
            Some(node) => node,
            None => continue,
        };

        // Check temporal filter
        if let Some(after) = request.created_after {
            if current_node.created_at < after {
                continue;
            }
        }

        // Add node if it matches kind filter
        let should_include = if depth == 0 && !request.include_start {
            false
        } else {
            match &request.kind_filter {
                Some(kinds) => kinds.contains(&current_node.kind),
                None => true,
            }
        };

        if should_include {
            result.nodes.insert(current_id, current_node.clone());
            result.depths.insert(current_id, depth);

            // Check result limit
            if let Some(limit) = request.limit {
                if result.nodes.len() >= limit {
                    result.truncated = true;
                    break;
                }
            }
        }

        // Check depth limit
        if let Some(max_depth) = request.max_depth {
            if depth >= max_depth {
                continue;
            }
        }

        // Get edges
        let edges = get_edges_for_node(storage, current_id, &request.direction)?;

        // Traverse edges (push in reverse order for consistent DFS ordering)
        let mut edge_neighbors = Vec::new();
        for edge in edges {
            // Apply filters (same as BFS)
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

            if let Some(after) = request.created_after {
                if edge.created_at < after {
                    continue;
                }
            }

            let next_id = if edge.from == current_id {
                edge.to
            } else {
                edge.from
            };

            if !visited.contains(&next_id) {
                edge_neighbors.push((next_id, edge));
            }
        }

        // Push to stack in reverse order
        for (next_id, edge) in edge_neighbors.into_iter().rev() {
            visited.insert(next_id);
            stack.push((next_id, depth + 1));

            candidate_edges.push(edge);
        }
    }

    // Post-pass: only include edges where both endpoints are in the result
    result.edges = candidate_edges
        .into_iter()
        .filter(|e| result.nodes.contains_key(&e.from) && result.nodes.contains_key(&e.to))
        .collect();

    Ok(result)
}

/// Weighted traversal (greedy best-first)
fn traverse_weighted<S: Storage>(
    storage: &S,
    request: TraversalRequest,
    budget: &TraversalBudget,
) -> Result<Subgraph> {
    let start_time = Instant::now();
    let mut result = Subgraph::new();
    let mut visited = HashSet::new();
    let mut queue = BinaryHeap::new();
    let mut candidate_edges = Vec::new();

    // Initialize with start nodes
    for node_id in &request.start {
        queue.push(WeightedNode {
            id: *node_id,
            depth: 0,
            weight: 1.0, // Start nodes have max weight
        });
        visited.insert(*node_id);
    }

    while let Some(WeightedNode { id: current_id, depth, weight: _ }) = queue.pop() {
        // Check budget
        if result.visited_count >= budget.max_visited {
            result.truncated = true;
            break;
        }
        if start_time.elapsed().as_millis() > budget.max_time_ms as u128 {
            result.truncated = true;
            break;
        }

        result.visited_count += 1;

        // Get current node
        let current_node = match storage.get_node(current_id)? {
            Some(node) => node,
            None => continue,
        };

        // Check temporal filter
        if let Some(after) = request.created_after {
            if current_node.created_at < after {
                continue;
            }
        }

        // Add node if it matches kind filter
        let should_include = if depth == 0 && !request.include_start {
            false
        } else {
            match &request.kind_filter {
                Some(kinds) => kinds.contains(&current_node.kind),
                None => true,
            }
        };

        if should_include {
            result.nodes.insert(current_id, current_node.clone());
            result.depths.insert(current_id, depth);

            // Check result limit
            if let Some(limit) = request.limit {
                if result.nodes.len() >= limit {
                    result.truncated = true;
                    break;
                }
            }
        }

        // Check depth limit
        if let Some(max_depth) = request.max_depth {
            if depth >= max_depth {
                continue;
            }
        }

        // Get edges
        let edges = get_edges_for_node(storage, current_id, &request.direction)?;

        // Traverse edges
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

            if let Some(after) = request.created_after {
                if edge.created_at < after {
                    continue;
                }
            }

            let next_id = if edge.from == current_id {
                edge.to
            } else {
                edge.from
            };

            if !visited.contains(&next_id) {
                visited.insert(next_id);
                queue.push(WeightedNode {
                    id: next_id,
                    depth: depth + 1,
                    weight: edge.weight,
                });
            }

            candidate_edges.push(edge);
        }
    }

    // Post-pass: only include edges where both endpoints are in the result
    result.edges = candidate_edges
        .into_iter()
        .filter(|e| result.nodes.contains_key(&e.from) && result.nodes.contains_key(&e.to))
        .collect();

    Ok(result)
}

/// Helper function to get edges for a node based on direction
fn get_edges_for_node<S: Storage>(
    storage: &S,
    node_id: NodeId,
    direction: &TraversalDirection,
) -> Result<Vec<Edge>> {
    match direction {
        TraversalDirection::Outgoing => storage.edges_from(node_id),
        TraversalDirection::Incoming => storage.edges_to(node_id),
        TraversalDirection::Both => {
            let mut edges = storage.edges_from(node_id)?;
            edges.extend(storage.edges_to(node_id)?);
            Ok(edges)
        }
    }
}
