use cortex_core::*;
use cortex_proto::*;
use prost_types::Timestamp;
use std::collections::HashMap;

/// Convert cortex Node to proto NodeResponse
pub fn node_to_response(node: &Node, edge_count: usize) -> NodeResponse {
    NodeResponse {
        id: node.id.to_string(),
        kind: format!("{:?}", node.kind),
        title: node.data.title.clone(),
        body: node.data.body.clone(),
        // Proto metadata is HashMap<String, String>; convert serde_json::Value to String
        metadata: node.data.metadata.iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
        tags: node.data.tags.clone(),
        importance: node.importance,
        source_agent: node.source.agent.clone(),
        source_session: node.source.session.clone(),
        source_channel: node.source.channel.clone(),
        access_count: node.access_count,
        created_at: Some(datetime_to_timestamp(node.created_at)),
        updated_at: Some(datetime_to_timestamp(node.updated_at)),
        has_embedding: node.embedding.is_some(),
        edge_count: edge_count as u32,
    }
}

/// Convert cortex Edge to proto EdgeResponse
pub fn edge_to_response(edge: &Edge) -> EdgeResponse {
    EdgeResponse {
        id: edge.id.to_string(),
        from_id: edge.from.to_string(),
        to_id: edge.to.to_string(),
        relation: format!("{:?}", edge.relation),
        weight: edge.weight,
        created_at: Some(datetime_to_timestamp(edge.created_at)),
        updated_at: Some(datetime_to_timestamp(edge.updated_at)),
    }
}

/// Convert chrono DateTime to protobuf Timestamp
pub fn datetime_to_timestamp(dt: chrono::DateTime<chrono::Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// Convert protobuf Timestamp to chrono DateTime
pub fn timestamp_to_datetime(ts: Timestamp) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
        .unwrap_or_else(|| chrono::Utc::now())
}

/// Parse NodeKind from string
pub fn parse_node_kind(s: &str) -> Result<NodeKind> {
    match s.to_lowercase().as_str() {
        "fact" => Ok(NodeKind::Fact),
        "decision" => Ok(NodeKind::Decision),
        "event" => Ok(NodeKind::Event),
        "observation" => Ok(NodeKind::Observation),
        "pattern" => Ok(NodeKind::Pattern),
        "agent" => Ok(NodeKind::Agent),
        "goal" => Ok(NodeKind::Goal),
        "preference" => Ok(NodeKind::Preference),
        _ => Err(CortexError::Validation(format!("Invalid NodeKind: {}", s))),
    }
}

/// Parse Relation from string
pub fn parse_relation(s: &str) -> Result<Relation> {
    match s.to_lowercase().as_str() {
        "informedby" | "informed_by" => Ok(Relation::InformedBy),
        "ledto" | "led_to" => Ok(Relation::LedTo),
        "dependson" | "depends_on" => Ok(Relation::DependsOn),
        "contradicts" => Ok(Relation::Contradicts),
        "supersedes" => Ok(Relation::Supersedes),
        "appliesto" | "applies_to" => Ok(Relation::AppliesTo),
        "relatedto" | "related_to" => Ok(Relation::RelatedTo),
        "instanceof" | "instance_of" => Ok(Relation::InstanceOf),
        _ => Err(CortexError::Validation(format!("Invalid Relation: {}", s))),
    }
}

/// Parse TraversalDirection from string
pub fn parse_direction(s: &str) -> TraversalDirection {
    match s.to_lowercase().as_str() {
        "outgoing" => TraversalDirection::Outgoing,
        "incoming" => TraversalDirection::Incoming,
        "both" => TraversalDirection::Both,
        _ => TraversalDirection::Both,
    }
}

/// Parse TraversalStrategy from string
pub fn parse_strategy(s: &str) -> TraversalStrategy {
    match s.to_lowercase().as_str() {
        "bfs" => TraversalStrategy::Bfs,
        "dfs" => TraversalStrategy::Dfs,
        "weighted" => TraversalStrategy::Weighted,
        _ => TraversalStrategy::Bfs,
    }
}

/// Parse VectorFilter from kind strings
pub fn parse_kind_filter(kinds: &[String]) -> Result<Vec<NodeKind>> {
    kinds.iter().map(|s| parse_node_kind(s)).collect()
}

/// Convert StorageStats to proto StatsResponse
pub fn stats_to_response(stats: StorageStats, db_size: u64) -> StatsResponse {
    let nodes_by_kind: HashMap<String, u64> = stats
        .node_counts_by_kind
        .into_iter()
        .map(|(k, v)| (format!("{:?}", k), v))
        .collect();

    let edges_by_relation: HashMap<String, u64> = stats
        .edge_counts_by_relation
        .into_iter()
        .map(|(r, v)| (format!("{:?}", r), v))
        .collect();

    StatsResponse {
        node_count: stats.node_count,
        edge_count: stats.edge_count,
        nodes_by_kind,
        edges_by_relation,
        db_size_bytes: db_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_source(agent: &str) -> Source {
        Source { agent: agent.to_string(), session: None, channel: None }
    }

    #[test]
    fn test_parse_node_kind_all_variants() {
        // Case-insensitive matching
        assert_eq!(parse_node_kind("fact").unwrap(), NodeKind::Fact);
        assert_eq!(parse_node_kind("Fact").unwrap(), NodeKind::Fact);
        assert_eq!(parse_node_kind("FACT").unwrap(), NodeKind::Fact);
        assert_eq!(parse_node_kind("decision").unwrap(), NodeKind::Decision);
        assert_eq!(parse_node_kind("event").unwrap(), NodeKind::Event);
        assert_eq!(parse_node_kind("observation").unwrap(), NodeKind::Observation);
        assert_eq!(parse_node_kind("pattern").unwrap(), NodeKind::Pattern);
        assert_eq!(parse_node_kind("agent").unwrap(), NodeKind::Agent);
        assert_eq!(parse_node_kind("goal").unwrap(), NodeKind::Goal);
        assert_eq!(parse_node_kind("preference").unwrap(), NodeKind::Preference);
    }

    #[test]
    fn test_parse_node_kind_invalid() {
        assert!(parse_node_kind("unknown").is_err());
        assert!(parse_node_kind("").is_err());
        assert!(parse_node_kind("facts").is_err());
    }

    #[test]
    fn test_parse_relation_all_variants() {
        assert_eq!(parse_relation("informedby").unwrap(), Relation::InformedBy);
        assert_eq!(parse_relation("informed_by").unwrap(), Relation::InformedBy);
        assert_eq!(parse_relation("ledto").unwrap(), Relation::LedTo);
        assert_eq!(parse_relation("led_to").unwrap(), Relation::LedTo);
        assert_eq!(parse_relation("dependson").unwrap(), Relation::DependsOn);
        assert_eq!(parse_relation("depends_on").unwrap(), Relation::DependsOn);
        assert_eq!(parse_relation("contradicts").unwrap(), Relation::Contradicts);
        assert_eq!(parse_relation("supersedes").unwrap(), Relation::Supersedes);
        assert_eq!(parse_relation("appliesto").unwrap(), Relation::AppliesTo);
        assert_eq!(parse_relation("applies_to").unwrap(), Relation::AppliesTo);
        assert_eq!(parse_relation("relatedto").unwrap(), Relation::RelatedTo);
        assert_eq!(parse_relation("related_to").unwrap(), Relation::RelatedTo);
        assert_eq!(parse_relation("instanceof").unwrap(), Relation::InstanceOf);
        assert_eq!(parse_relation("instance_of").unwrap(), Relation::InstanceOf);
    }

    #[test]
    fn test_parse_relation_invalid() {
        assert!(parse_relation("unknown").is_err());
        assert!(parse_relation("").is_err());
        assert!(parse_relation("supports").is_err());
    }

    #[test]
    fn test_parse_direction_known_values() {
        assert!(matches!(parse_direction("outgoing"), TraversalDirection::Outgoing));
        assert!(matches!(parse_direction("incoming"), TraversalDirection::Incoming));
        assert!(matches!(parse_direction("both"), TraversalDirection::Both));
        assert!(matches!(parse_direction("OUTGOING"), TraversalDirection::Outgoing));
    }

    #[test]
    fn test_parse_direction_defaults_to_both() {
        assert!(matches!(parse_direction("unknown"), TraversalDirection::Both));
        assert!(matches!(parse_direction(""), TraversalDirection::Both));
    }

    #[test]
    fn test_parse_strategy_known_values() {
        assert!(matches!(parse_strategy("bfs"), TraversalStrategy::Bfs));
        assert!(matches!(parse_strategy("dfs"), TraversalStrategy::Dfs));
        assert!(matches!(parse_strategy("weighted"), TraversalStrategy::Weighted));
    }

    #[test]
    fn test_parse_strategy_defaults_to_bfs() {
        assert!(matches!(parse_strategy("unknown"), TraversalStrategy::Bfs));
        assert!(matches!(parse_strategy(""), TraversalStrategy::Bfs));
    }

    #[test]
    fn test_datetime_timestamp_roundtrip() {
        let now = chrono::Utc::now();
        // Truncate to second precision (proto Timestamp is seconds + nanos)
        let ts = datetime_to_timestamp(now);
        let restored = timestamp_to_datetime(ts);
        let diff_ms = (now - restored).num_milliseconds().abs();
        assert!(diff_ms < 1, "Timestamp roundtrip should preserve millisecond precision");
    }

    #[test]
    fn test_node_to_response_basic_fields() {
        let node = Node::new(
            NodeKind::Fact,
            "Test Title".to_string(),
            "Test Body".to_string(),
            make_source("test-agent"),
            0.75,
        );
        let response = node_to_response(&node, 3);

        assert_eq!(response.id, node.id.to_string());
        assert_eq!(response.title, "Test Title");
        assert_eq!(response.body, "Test Body");
        assert_eq!(response.importance, 0.75);
        assert_eq!(response.source_agent, "test-agent");
        assert_eq!(response.edge_count, 3);
        assert!(!response.has_embedding);
    }

    #[test]
    fn test_node_to_response_with_embedding() {
        let mut node = Node::new(
            NodeKind::Decision,
            "Decision".to_string(),
            "Body".to_string(),
            make_source("agent"),
            0.5,
        );
        node.embedding = Some(vec![0.1, 0.2, 0.3]);
        let response = node_to_response(&node, 0);
        assert!(response.has_embedding);
    }

    #[test]
    fn test_node_to_response_kind_string() {
        let node = Node::new(NodeKind::Pattern, "P".to_string(), "".to_string(), make_source("a"), 0.5);
        let response = node_to_response(&node, 0);
        assert_eq!(response.kind, "Pattern");
    }

    #[test]
    fn test_edge_to_response_fields() {
        use uuid::Uuid;
        let from = Uuid::now_v7();
        let to = Uuid::now_v7();
        // Edge::new doesn't validate nodes exist (that's put_edge's job)
        let edge = Edge::new(
            from, to, Relation::RelatedTo, 0.7,
            EdgeProvenance::AutoSimilarity { score: 0.85 },
        );
        let response = edge_to_response(&edge);

        assert_eq!(response.id, edge.id.to_string());
        assert_eq!(response.from_id, from.to_string());
        assert_eq!(response.to_id, to.to_string());
        assert_eq!(response.relation, "RelatedTo");
        assert!((response.weight - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_stats_to_response() {
        use std::collections::HashMap;

        let mut by_kind = HashMap::new();
        by_kind.insert(NodeKind::Fact, 10u64);
        by_kind.insert(NodeKind::Decision, 5u64);

        let mut by_relation = HashMap::new();
        by_relation.insert(Relation::RelatedTo, 20u64);

        let stats = StorageStats {
            node_count: 15,
            edge_count: 20,
            node_counts_by_kind: by_kind,
            edge_counts_by_relation: by_relation,
            db_size_bytes: 1024,
            oldest_node: None,
            newest_node: None,
        };

        // db_size parameter overrides stats.db_size_bytes
        let response = stats_to_response(stats, 2048);
        assert_eq!(response.node_count, 15);
        assert_eq!(response.edge_count, 20);
        assert_eq!(response.db_size_bytes, 2048);
        // NodeKind is formatted as Debug string ("Fact", not "fact")
        assert!(response.nodes_by_kind.contains_key("Fact"));
        assert!(response.nodes_by_kind.contains_key("Decision"));
        assert!(response.edges_by_relation.contains_key("RelatedTo"));
    }

    #[test]
    fn test_parse_kind_filter_batch() {
        let kinds = vec!["fact".to_string(), "decision".to_string()];
        let result = parse_kind_filter(&kinds).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&NodeKind::Fact));
        assert!(result.contains(&NodeKind::Decision));
    }

    #[test]
    fn test_parse_kind_filter_invalid_fails() {
        let kinds = vec!["fact".to_string(), "invalid".to_string()];
        assert!(parse_kind_filter(&kinds).is_err());
    }
}
