//! Entity layer: entity extraction, co-occurrence detection, and promotion.
//!
//! Entities are hub nodes (`kind: "entity"`) that unify knowledge about the same
//! real-world thing across multiple agents. This module provides:
//! - Extraction of entity mentions from node metadata and tags
//! - Co-occurrence edge creation when different agents reference the same entity
//! - Periodic promotion of frequently-mentioned entities to first-class entity nodes

use crate::error::Result;
use crate::linker::ProposedEdge;
use crate::storage::{NodeFilter, Storage};
use crate::types::{Edge, EdgeProvenance, Node, NodeId, NodeKind, Relation, Source};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

/// Normalize an entity string: lowercase, split on whitespace, join with hyphens,
/// strip leading articles, keep only alphanumeric + hyphens.
pub fn normalize_entity(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let trimmed = lower.trim();
    let without_articles = trimmed
        .strip_prefix("the ")
        .or_else(|| trimmed.strip_prefix("a "))
        .or_else(|| trimmed.strip_prefix("an "))
        .unwrap_or(trimmed);

    let joined: String = without_articles
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    joined
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// Extract normalised entity strings from a node's metadata and tags.
///
/// Sources:
/// 1. `metadata.entities` — JSON array of entity name strings
/// 2. Tags prefixed with `entity-` (e.g. `entity-company-x`)
pub fn extract_entities(node: &Node) -> Vec<String> {
    let mut entities = Vec::new();

    // 1. Metadata: if the node declares entities, use them
    if let Some(Value::Array(arr)) = node.data.metadata.get("entities") {
        for v in arr {
            if let Value::String(s) = v {
                let normalized = normalize_entity(s);
                if !normalized.is_empty() {
                    entities.push(normalized);
                }
            }
        }
    }

    // 2. Tags prefixed with "entity-" are entity references
    for tag in &node.data.tags {
        if let Some(name) = tag.strip_prefix("entity-") {
            let normalized = normalize_entity(name);
            if !normalized.is_empty() {
                entities.push(normalized);
            }
        }
    }

    entities.sort();
    entities.dedup();
    entities
}

/// Generate `shared_entity` edges for two nodes from different agents
/// that mention the same entity.
pub fn entity_cooccurrence_edges(node: &Node, neighbor: &Node) -> Vec<ProposedEdge> {
    let mut edges = Vec::new();

    // Only fire across different agents
    if node.source.agent == neighbor.source.agent {
        return edges;
    }

    // Self-edge check
    if node.id == neighbor.id {
        return edges;
    }

    let node_entities = extract_entities(node);
    let neighbor_entities = extract_entities(neighbor);

    let shared: Vec<&String> = node_entities
        .iter()
        .filter(|e| neighbor_entities.contains(e))
        .collect();

    for entity_name in shared {
        edges.push(ProposedEdge {
            from: node.id,
            to: neighbor.id,
            relation: Relation::new("shared_entity").unwrap(),
            weight: 0.8,
            provenance: EdgeProvenance::AutoStructural {
                rule: format!("entity_cooccurrence:{}", entity_name),
            },
            metadata: HashMap::from([("entity".to_string(), entity_name.to_string())]),
        });
    }

    edges
}

/// Promote frequently-mentioned entities to first-class entity nodes.
///
/// Scans all nodes, extracts entity mentions, and creates entity nodes for any
/// entity mentioned by `>= min_agents` distinct agents. Also creates `references`
/// edges from mentioning nodes to the new entity node.
pub fn promote_entities<S: Storage>(storage: &S, min_agents: usize) -> Result<u64> {
    let all_nodes = storage.list_nodes(NodeFilter::new())?;

    // Track which agents mention each entity, and which nodes reference each
    let mut entity_agents: HashMap<String, HashSet<String>> = HashMap::new();
    let mut entity_node_ids: HashMap<String, Vec<NodeId>> = HashMap::new();

    for node in &all_nodes {
        if node.deleted || node.kind.as_str() == "entity" {
            continue;
        }
        let entities = extract_entities(node);
        for entity_name in entities {
            entity_agents
                .entry(entity_name.clone())
                .or_default()
                .insert(node.source.agent.clone());
            entity_node_ids
                .entry(entity_name)
                .or_default()
                .push(node.id);
        }
    }

    let mut promoted = 0;
    for (entity_name, agents) in &entity_agents {
        if agents.len() < min_agents {
            continue;
        }

        // Check if entity node already exists (by tag)
        let existing = storage.list_nodes(
            NodeFilter::new()
                .with_kinds(vec![NodeKind::new("entity").unwrap()])
                .with_tags(vec![format!("entity-{}", entity_name)]),
        )?;

        if !existing.is_empty() {
            continue;
        }

        // Create entity node.
        // Note: entity_type is stored as a tag (not metadata) because
        // serde_json::Value cannot roundtrip through bincode storage.
        let mut entity_node = Node::new(
            NodeKind::new("entity").unwrap(),
            entity_name.replace('-', " "),
            String::new(),
            Source {
                agent: "system".into(),
                session: None,
                channel: None,
            },
            0.8,
        );
        entity_node
            .data
            .tags
            .push(format!("entity-{}", entity_name));
        entity_node
            .data
            .tags
            .push("entity-type-auto-promoted".to_string());

        storage.put_node(&entity_node)?;

        // Create references edges from mentioning nodes to entity
        if let Some(node_ids) = entity_node_ids.get(entity_name) {
            for node_id in node_ids {
                let edge = Edge::new(
                    *node_id,
                    entity_node.id,
                    Relation::new("references").unwrap(),
                    0.7,
                    EdgeProvenance::AutoStructural {
                        rule: format!("entity_promotion:{}", entity_name),
                    },
                );
                match storage.put_edge(&edge) {
                    Ok(()) => {}
                    Err(crate::error::CortexError::DuplicateEdge { .. }) => continue,
                    Err(crate::error::CortexError::InvalidEdge { .. }) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        promoted += 1;
    }

    Ok(promoted)
}

/// Migrate an agent node to an entity node with `entity_type: "agent"`.
///
/// Uses a tag (`entity-type-agent`) for the entity type since
/// `serde_json::Value` metadata cannot roundtrip through bincode storage.
pub fn migrate_agent_to_entity(node: &mut Node) {
    if node.kind.as_str() == "agent" {
        node.kind = NodeKind::new("entity").unwrap();
        if !node.data.tags.contains(&"entity-type-agent".to_string()) {
            node.data.tags.push("entity-type-agent".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Source;

    fn test_source(agent: &str) -> Source {
        Source {
            agent: agent.to_string(),
            session: None,
            channel: None,
        }
    }

    // --- normalize_entity ---

    #[test]
    fn test_normalize_entity_basic() {
        assert_eq!(normalize_entity("Company X"), "company-x");
    }

    #[test]
    fn test_normalize_entity_strips_articles() {
        assert_eq!(normalize_entity("The Company X SDK"), "company-x-sdk");
    }

    #[test]
    fn test_normalize_entity_special_chars() {
        assert_eq!(normalize_entity("Company X, Inc."), "company-x-inc");
    }

    #[test]
    fn test_normalize_entity_already_normalized() {
        assert_eq!(normalize_entity("company-x"), "company-x");
    }

    #[test]
    fn test_normalize_entity_whitespace() {
        assert_eq!(normalize_entity("  Company   X  "), "company-x");
    }

    #[test]
    fn test_normalize_entity_empty() {
        assert_eq!(normalize_entity(""), "");
    }

    // --- extract_entities ---

    #[test]
    fn test_extract_entities_from_metadata() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Test".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.metadata.insert(
            "entities".to_string(),
            Value::Array(vec![
                Value::String("Company X".to_string()),
                Value::String("Project Y".to_string()),
            ]),
        );

        let entities = extract_entities(&node);
        assert_eq!(entities, vec!["company-x", "project-y"]);
    }

    #[test]
    fn test_extract_entities_from_tags() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Test".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.tags = vec![
            "entity-company-x".to_string(),
            "other-tag".to_string(),
            "entity-project-y".to_string(),
        ];

        let entities = extract_entities(&node);
        assert_eq!(entities, vec!["company-x", "project-y"]);
    }

    #[test]
    fn test_extract_entities_deduplicates() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Test".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.metadata.insert(
            "entities".to_string(),
            Value::Array(vec![Value::String("Company X".to_string())]),
        );
        node.data.tags = vec!["entity-company-x".to_string()];

        let entities = extract_entities(&node);
        assert_eq!(entities, vec!["company-x"]);
    }

    #[test]
    fn test_extract_entities_empty() {
        let node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Test".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        let entities = extract_entities(&node);
        assert!(entities.is_empty());
    }

    // --- entity_cooccurrence_edges ---

    #[test]
    fn test_cooccurrence_different_agents_shared_entity() {
        let mut node = Node::new(
            NodeKind::new("observation").unwrap(),
            "Observed Company X pivot".into(),
            String::new(),
            test_source("strategy-agent"),
            0.5,
        );
        node.data.tags = vec!["entity-company-x".to_string()];

        let mut neighbor = Node::new(
            NodeKind::new("fact").unwrap(),
            "Evaluated Company X SDK".into(),
            String::new(),
            test_source("engineering-agent"),
            0.5,
        );
        neighbor.data.metadata.insert(
            "entities".to_string(),
            Value::Array(vec![Value::String("Company X".to_string())]),
        );

        let edges = entity_cooccurrence_edges(&node, &neighbor);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation, Relation::new("shared_entity").unwrap());
        assert_eq!(edges[0].metadata.get("entity").unwrap(), "company-x");
    }

    #[test]
    fn test_cooccurrence_same_agent_no_edges() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "A".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.tags = vec!["entity-company-x".to_string()];

        let mut neighbor = Node::new(
            NodeKind::new("fact").unwrap(),
            "B".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        neighbor.data.tags = vec!["entity-company-x".to_string()];

        let edges = entity_cooccurrence_edges(&node, &neighbor);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_cooccurrence_no_shared_entities() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "A".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.tags = vec!["entity-company-x".to_string()];

        let mut neighbor = Node::new(
            NodeKind::new("fact").unwrap(),
            "B".into(),
            String::new(),
            test_source("agent-2"),
            0.5,
        );
        neighbor.data.tags = vec!["entity-company-y".to_string()];

        let edges = entity_cooccurrence_edges(&node, &neighbor);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_cooccurrence_multiple_shared_entities() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "A".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node.data.tags = vec![
            "entity-company-x".to_string(),
            "entity-project-y".to_string(),
        ];

        let mut neighbor = Node::new(
            NodeKind::new("fact").unwrap(),
            "B".into(),
            String::new(),
            test_source("agent-2"),
            0.5,
        );
        neighbor.data.tags = vec![
            "entity-company-x".to_string(),
            "entity-project-y".to_string(),
        ];

        let edges = entity_cooccurrence_edges(&node, &neighbor);
        assert_eq!(edges.len(), 2);
    }

    // --- migrate_agent_to_entity ---

    #[test]
    fn test_migrate_agent_to_entity() {
        let mut node = Node::new(
            NodeKind::new("agent").unwrap(),
            "research-agent".into(),
            String::new(),
            test_source("system"),
            1.0,
        );

        migrate_agent_to_entity(&mut node);

        assert_eq!(node.kind.as_str(), "entity");
        assert!(node.data.tags.contains(&"entity-type-agent".to_string()));
    }

    #[test]
    fn test_migrate_agent_idempotent() {
        let mut node = Node::new(
            NodeKind::new("agent").unwrap(),
            "research-agent".into(),
            String::new(),
            test_source("system"),
            1.0,
        );

        migrate_agent_to_entity(&mut node);
        migrate_agent_to_entity(&mut node); // second call should be no-op (already entity)

        assert_eq!(node.kind.as_str(), "entity");
        assert_eq!(
            node.data
                .tags
                .iter()
                .filter(|t| *t == "entity-type-agent")
                .count(),
            1
        );
    }

    #[test]
    fn test_migrate_non_agent_no_op() {
        let mut node = Node::new(
            NodeKind::new("fact").unwrap(),
            "Some fact".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );

        migrate_agent_to_entity(&mut node);

        assert_eq!(node.kind.as_str(), "fact");
        assert!(!node.data.tags.contains(&"entity-type-agent".to_string()));
    }

    // --- promote_entities (requires storage) ---

    #[test]
    fn test_promote_entities_creates_entity_node() {
        use crate::storage::RedbStorage;
        use std::sync::Arc;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("entity_promote_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Create nodes from two different agents mentioning the same entity (via tags)
        let mut node1 = Node::new(
            NodeKind::new("observation").unwrap(),
            "Company X is pivoting".into(),
            String::new(),
            test_source("strategy-agent"),
            0.5,
        );
        node1.data.tags = vec!["entity-company-x".to_string()];
        storage.put_node(&node1).unwrap();

        let mut node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Company X SDK evaluated".into(),
            String::new(),
            test_source("engineering-agent"),
            0.5,
        );
        node2.data.tags = vec!["entity-company-x".to_string()];
        storage.put_node(&node2).unwrap();

        // Promote with min_agents = 2
        let promoted = promote_entities(storage.as_ref(), 2).unwrap();
        assert_eq!(promoted, 1);

        // Verify entity node was created
        let entity_nodes = storage
            .list_nodes(
                NodeFilter::new().with_kinds(vec![NodeKind::new("entity").unwrap()]),
            )
            .unwrap();
        assert_eq!(entity_nodes.len(), 1);
        assert_eq!(entity_nodes[0].data.title, "company x");
        assert!(entity_nodes[0]
            .data
            .tags
            .contains(&"entity-type-auto-promoted".to_string()));
        assert!(entity_nodes[0]
            .data
            .tags
            .contains(&"entity-company-x".to_string()));

        // Verify references edges were created
        let edges_to_entity = storage.edges_to(entity_nodes[0].id).unwrap();
        assert_eq!(edges_to_entity.len(), 2);
        for edge in &edges_to_entity {
            assert_eq!(edge.relation, Relation::new("references").unwrap());
        }
    }

    #[test]
    fn test_promote_entities_threshold_not_met() {
        use crate::storage::RedbStorage;
        use std::sync::Arc;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("entity_no_promote_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        // Only one agent mentions the entity
        let mut node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "Company X info".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node1.data.tags = vec!["entity-company-x".to_string()];
        storage.put_node(&node1).unwrap();

        let promoted = promote_entities(storage.as_ref(), 2).unwrap();
        assert_eq!(promoted, 0);

        let entity_nodes = storage
            .list_nodes(
                NodeFilter::new().with_kinds(vec![NodeKind::new("entity").unwrap()]),
            )
            .unwrap();
        assert!(entity_nodes.is_empty());
    }

    #[test]
    fn test_promote_entities_idempotent() {
        use crate::storage::RedbStorage;
        use std::sync::Arc;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("entity_idempotent_test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());

        let mut node1 = Node::new(
            NodeKind::new("fact").unwrap(),
            "A".into(),
            String::new(),
            test_source("agent-1"),
            0.5,
        );
        node1.data.tags = vec!["entity-company-x".to_string()];
        storage.put_node(&node1).unwrap();

        let mut node2 = Node::new(
            NodeKind::new("fact").unwrap(),
            "B".into(),
            String::new(),
            test_source("agent-2"),
            0.5,
        );
        node2.data.tags = vec!["entity-company-x".to_string()];
        storage.put_node(&node2).unwrap();

        // First promotion
        let promoted1 = promote_entities(storage.as_ref(), 2).unwrap();
        assert_eq!(promoted1, 1);

        // Second promotion — should not create duplicate
        let promoted2 = promote_entities(storage.as_ref(), 2).unwrap();
        assert_eq!(promoted2, 0);

        let entity_nodes = storage
            .list_nodes(
                NodeFilter::new().with_kinds(vec![NodeKind::new("entity").unwrap()]),
            )
            .unwrap();
        assert_eq!(entity_nodes.len(), 1);
    }
}
