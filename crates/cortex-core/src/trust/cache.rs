//! Source reliability cache — per-agent reliability scores.

use crate::error::Result;
use crate::storage::{NodeFilter, Storage};

/// Compute source reliability for a given agent.
///
/// reliability = 1.0 - (superseded + contradicted) / max(total, 1)
/// Clamped to [0.2, 1.0] — even unreliable sources aren't zero.
pub fn compute_source_reliability<S: Storage>(storage: &S, agent_id: &str) -> Result<f32> {
    let agent_nodes =
        storage.list_nodes(NodeFilter::new().with_source_agent(agent_id.to_string()))?;

    let total = agent_nodes.len();
    if total == 0 {
        // No history → assume neutral reliability.
        return Ok(1.0);
    }

    let mut superseded: usize = 0;
    let mut contradicted: usize = 0;

    for node in &agent_nodes {
        if node.deleted {
            continue;
        }
        let incoming = storage.edges_to(node.id)?;
        for edge in &incoming {
            if edge.relation.as_str() == "supersedes" {
                superseded += 1;
                break; // count once per node
            }
        }
        let all_edges: Vec<_> = incoming
            .iter()
            .chain(storage.edges_from(node.id)?.iter())
            .cloned()
            .collect();
        for edge in &all_edges {
            if edge.relation.as_str() == "contradicts" {
                contradicted += 1;
                break; // count once per node
            }
        }
    }

    let reliability = 1.0 - (superseded + contradicted) as f32 / total.max(1) as f32;
    Ok(reliability.clamp(0.2, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use crate::types::{Edge, EdgeProvenance, Node, NodeKind, Relation, Source};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_storage() -> (Arc<RedbStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let storage = RedbStorage::open(dir.path().join("test.redb")).unwrap();
        (Arc::new(storage), dir)
    }

    fn make_node(agent: &str, title: &str) -> Node {
        Node::new(
            NodeKind::new("fact").unwrap(),
            title.to_string(),
            "Body".to_string(),
            Source {
                agent: agent.to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    #[test]
    fn test_no_history_returns_full_reliability() {
        let (storage, _dir) = test_storage();
        let r = compute_source_reliability(storage.as_ref(), "unknown-agent").unwrap();
        assert_eq!(r, 1.0);
    }

    #[test]
    fn test_clean_agent_high_reliability() {
        let (storage, _dir) = test_storage();
        for i in 0..5 {
            let node = make_node("clean-agent", &format!("Fact {i}"));
            storage.put_node(&node).unwrap();
        }

        let r = compute_source_reliability(storage.as_ref(), "clean-agent").unwrap();
        assert_eq!(r, 1.0);
    }

    #[test]
    fn test_superseded_nodes_reduce_reliability() {
        let (storage, _dir) = test_storage();

        let n1 = make_node("flaky-agent", "Old fact");
        let n2 = make_node("flaky-agent", "Still good");
        let n3 = make_node("other-agent", "Better fact");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();
        storage.put_node(&n3).unwrap();

        // n3 supersedes n1
        let edge = Edge::new(
            n3.id,
            n1.id,
            Relation::new("supersedes").unwrap(),
            1.0,
            EdgeProvenance::Manual {
                created_by: "other-agent".to_string(),
            },
        );
        storage.put_edge(&edge).unwrap();

        let r = compute_source_reliability(storage.as_ref(), "flaky-agent").unwrap();
        // 2 total nodes, 1 superseded → 1.0 - 1/2 = 0.5
        assert!((r - 0.5).abs() < 0.001, "Expected ~0.5, got {r}");
    }

    #[test]
    fn test_reliability_floors_at_0_2() {
        let (storage, _dir) = test_storage();

        // Create 2 nodes, both get contradicted
        let n1 = make_node("bad-agent", "Wrong 1");
        let n2 = make_node("bad-agent", "Wrong 2");
        let n3 = make_node("other-agent", "Correction 1");
        let n4 = make_node("other-agent", "Correction 2");
        storage.put_node(&n1).unwrap();
        storage.put_node(&n2).unwrap();
        storage.put_node(&n3).unwrap();
        storage.put_node(&n4).unwrap();

        for (a, b) in [(n1.id, n3.id), (n2.id, n4.id)] {
            let edge = Edge::new(
                a,
                b,
                Relation::new("contradicts").unwrap(),
                1.0,
                EdgeProvenance::AutoContradiction {
                    reason: "test".to_string(),
                },
            );
            storage.put_edge(&edge).unwrap();
        }

        let r = compute_source_reliability(storage.as_ref(), "bad-agent").unwrap();
        // 2 nodes, 2 contradicted → 1.0 - 2/2 = 0.0, clamped to 0.2
        assert!((r - 0.2).abs() < 0.001, "Should floor at 0.2, got {r}");
    }
}
