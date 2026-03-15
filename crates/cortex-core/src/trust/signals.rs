//! Individual trust signal computations.

use crate::error::Result;
use crate::storage::Storage;
use crate::types::{Edge, EdgeProvenance, Node};
use chrono::Utc;
use std::collections::HashSet;

use super::TrustConfig;

/// Corroboration: how many independent agents stored semantically similar facts?
///
/// Returns (normalised signal in [0, 1], list of corroborating agent IDs).
pub fn corroboration<S: Storage>(
    storage: &S,
    node: &Node,
    config: &TrustConfig,
) -> Result<(f32, Vec<String>)> {
    let node_agent = &node.source.agent;
    let mut corroborating_agents: HashSet<String> = HashSet::new();

    // Check both incoming and outgoing edges for auto-similarity links.
    let edges_out = storage.edges_from(node.id)?;
    let edges_in = storage.edges_to(node.id)?;

    let all_edges: Vec<&Edge> = edges_out.iter().chain(edges_in.iter()).collect();

    for edge in all_edges {
        // Only count auto-similarity edges above the min weight threshold.
        if !matches!(edge.provenance, EdgeProvenance::AutoSimilarity { .. }) {
            continue;
        }
        if edge.weight < config.corroboration_min_weight {
            continue;
        }

        // Find the OTHER node's agent.
        let other_id = if edge.from == node.id {
            edge.to
        } else {
            edge.from
        };

        if let Ok(Some(other_node)) = storage.get_node(other_id) {
            if other_node.source.agent != *node_agent && !other_node.deleted {
                corroborating_agents.insert(other_node.source.agent.clone());
            }
        }
    }

    let count = corroborating_agents.len() as f32;
    let normalised = (count / config.corroboration_saturation as f32).min(1.0);

    Ok((normalised, corroborating_agents.into_iter().collect()))
}

/// Contradiction: penalty for unresolved contradictions.
///
/// Returns (signal value where 1.0 = no contradictions, count of contradiction edges).
pub fn contradiction<S: Storage>(
    storage: &S,
    node: &Node,
    config: &TrustConfig,
) -> Result<(f32, u32)> {
    let edges_out = storage.edges_from(node.id)?;
    let edges_in = storage.edges_to(node.id)?;

    let count = edges_out
        .iter()
        .chain(edges_in.iter())
        .filter(|e| e.relation.as_str() == "contradicts")
        .count() as u32;

    let penalty = (count as f32 * config.contradiction_weight).min(1.0);
    let signal = 1.0 - penalty;

    Ok((signal, count))
}

/// Access reinforcement: frequently retrieved nodes that are never corrected
/// are implicitly validated.
pub fn access_reinforcement(node: &Node, config: &TrustConfig) -> f32 {
    (node.access_count as f32 / config.access_saturation as f32).min(1.0)
}

/// Freshness: recently accessed nodes are more likely to be current.
pub fn freshness(node: &Node, config: &TrustConfig) -> f32 {
    let now = Utc::now();
    let days_idle = (now - node.last_accessed_at).num_seconds() as f64 / 86400.0;
    (1.0 - days_idle / config.freshness_halflife).max(0.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, NodeKind, Relation, Source};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_storage() -> (Arc<RedbStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = RedbStorage::open(dir.path().join("test.redb")).unwrap();
        (Arc::new(storage), dir)
    }

    fn make_node(agent: &str) -> Node {
        Node::new(
            NodeKind::new("fact").unwrap(),
            format!("Fact by {agent}"),
            "Body".to_string(),
            Source {
                agent: agent.to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    fn make_similarity_edge(from: uuid::Uuid, to: uuid::Uuid, weight: f32) -> Edge {
        Edge::new(
            from,
            to,
            Relation::new("related_to").unwrap(),
            weight,
            EdgeProvenance::AutoSimilarity { score: weight },
        )
    }

    fn make_contradiction_edge(from: uuid::Uuid, to: uuid::Uuid) -> Edge {
        Edge::new(
            from,
            to,
            Relation::new("contradicts").unwrap(),
            1.0,
            EdgeProvenance::AutoContradiction {
                reason: "test".to_string(),
            },
        )
    }

    #[test]
    fn test_corroboration_increases_with_independent_agents() {
        let (storage, _dir) = test_storage();
        let config = TrustConfig::default();

        let node_a = make_node("agent-a");
        let node_b = make_node("agent-b");
        let node_c = make_node("agent-c");
        storage.put_node(&node_a).unwrap();
        storage.put_node(&node_b).unwrap();
        storage.put_node(&node_c).unwrap();

        // No edges yet: corroboration = 0
        let (score0, agents0) = corroboration(storage.as_ref(), &node_a, &config).unwrap();
        assert_eq!(score0, 0.0);
        assert!(agents0.is_empty());

        // Add similarity edge from agent-b
        let edge_ab = make_similarity_edge(node_a.id, node_b.id, 0.8);
        storage.put_edge(&edge_ab).unwrap();

        let (score1, agents1) = corroboration(storage.as_ref(), &node_a, &config).unwrap();
        assert!(score1 > 0.0);
        assert!(agents1.contains(&"agent-b".to_string()));

        // Add another from agent-c
        let edge_ac = make_similarity_edge(node_a.id, node_c.id, 0.7);
        storage.put_edge(&edge_ac).unwrap();

        let (score2, _agents2) = corroboration(storage.as_ref(), &node_a, &config).unwrap();
        assert!(score2 > score1, "More corroboration should increase score");
    }

    #[test]
    fn test_corroboration_ignores_same_agent() {
        let (storage, _dir) = test_storage();
        let config = TrustConfig::default();

        let node_a1 = make_node("agent-a");
        let node_a2 = make_node("agent-a");
        storage.put_node(&node_a1).unwrap();
        storage.put_node(&node_a2).unwrap();

        let edge = make_similarity_edge(node_a1.id, node_a2.id, 0.9);
        storage.put_edge(&edge).unwrap();

        let (score, agents) = corroboration(storage.as_ref(), &node_a1, &config).unwrap();
        assert_eq!(score, 0.0, "Same agent should not count as corroboration");
        assert!(agents.is_empty());
    }

    #[test]
    fn test_corroboration_ignores_low_weight_edges() {
        let (storage, _dir) = test_storage();
        let config = TrustConfig::default(); // min_weight = 0.6

        let node_a = make_node("agent-a");
        let node_b = make_node("agent-b");
        storage.put_node(&node_a).unwrap();
        storage.put_node(&node_b).unwrap();

        let edge = make_similarity_edge(node_a.id, node_b.id, 0.3); // below threshold
        storage.put_edge(&edge).unwrap();

        let (score, _) = corroboration(storage.as_ref(), &node_a, &config).unwrap();
        assert_eq!(score, 0.0, "Low weight edges should not count");
    }

    #[test]
    fn test_contradiction_reduces_score() {
        let (storage, _dir) = test_storage();
        let config = TrustConfig::default();

        let node_a = make_node("agent-a");
        let node_b = make_node("agent-b");
        storage.put_node(&node_a).unwrap();
        storage.put_node(&node_b).unwrap();

        // No contradictions
        let (score0, count0) = contradiction(storage.as_ref(), &node_a, &config).unwrap();
        assert_eq!(score0, 1.0);
        assert_eq!(count0, 0);

        // One contradiction
        let edge = make_contradiction_edge(node_a.id, node_b.id);
        storage.put_edge(&edge).unwrap();

        let (score1, count1) = contradiction(storage.as_ref(), &node_a, &config).unwrap();
        assert!(score1 < 1.0, "Contradiction should reduce score");
        assert_eq!(count1, 1);
        // With default weight 0.3: 1.0 - 0.3 = 0.7
        assert!((score1 - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_access_reinforcement_signal() {
        let config = TrustConfig::default(); // saturation = 20
        let mut node = make_node("agent-a");

        node.access_count = 0;
        assert_eq!(access_reinforcement(&node, &config), 0.0);

        node.access_count = 10;
        assert!((access_reinforcement(&node, &config) - 0.5).abs() < 0.001);

        node.access_count = 20;
        assert!((access_reinforcement(&node, &config) - 1.0).abs() < 0.001);

        node.access_count = 100;
        assert_eq!(
            access_reinforcement(&node, &config),
            1.0,
            "Should cap at 1.0"
        );
    }

    #[test]
    fn test_freshness_signal() {
        let config = TrustConfig::default(); // halflife = 90 days
        let mut node = make_node("agent-a");

        // Just accessed = freshness ~1.0
        node.last_accessed_at = Utc::now();
        let f = freshness(&node, &config);
        assert!(f > 0.99, "Just accessed should be near 1.0, got {f}");

        // 45 days ago = ~0.5
        node.last_accessed_at = Utc::now() - chrono::Duration::days(45);
        let f = freshness(&node, &config);
        assert!((f - 0.5).abs() < 0.02, "45 days should be ~0.5, got {f}");

        // 90 days ago = 0.0
        node.last_accessed_at = Utc::now() - chrono::Duration::days(90);
        let f = freshness(&node, &config);
        assert!(f.abs() < 0.02, "90 days should be ~0.0, got {f}");

        // 180 days ago = 0.0 (clamped)
        node.last_accessed_at = Utc::now() - chrono::Duration::days(180);
        let f = freshness(&node, &config);
        assert_eq!(f, 0.0, "Should clamp at 0.0");
    }
}
