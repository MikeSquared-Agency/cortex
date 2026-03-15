//! Well-known metadata conventions for Cortex nodes and edges.
//!
//! These are **documented conventions**, not schema constraints. Validation
//! helpers return warnings (never errors) and never block storage.

use crate::types::Node;
use serde_json::Value;

/// Well-known entity types. Not an enum — just constants for documentation
/// and programmatic use. Custom entity types are allowed.
pub mod entity_types {
    pub const AGENT: &str = "agent";
    pub const COMPANY: &str = "company";
    pub const PERSON: &str = "person";
    pub const TECHNOLOGY: &str = "technology";
    pub const PROJECT: &str = "project";
    pub const LOCATION: &str = "location";
    pub const PRODUCT: &str = "product";

    /// All well-known entity type values.
    pub const ALL: &[&str] = &[
        AGENT, COMPANY, PERSON, TECHNOLOGY, PROJECT, LOCATION, PRODUCT,
    ];
}

/// Well-known node metadata keys.
pub mod node_keys {
    pub const ENTITY_TYPE: &str = "entity_type";
    pub const ALIASES: &str = "aliases";
    pub const PARENT_AGENT: &str = "parent_agent";
    pub const TASK_ID: &str = "task_id";
    pub const SOURCE_URL: &str = "source_url";
    pub const CONTENT_TYPE: &str = "content_type";
    pub const LANGUAGE: &str = "language";
    pub const EXPIRES_REASON: &str = "expires_reason";
}

/// Well-known edge metadata keys.
pub mod edge_keys {
    pub const ENTITY: &str = "entity";
    pub const SIMILARITY_CONTEXT: &str = "similarity_context";
    pub const RULE_VERSION: &str = "rule_version";
}

/// Check whether a node's metadata uses well-known keys correctly.
/// Returns warnings, not errors. Never blocks storage.
pub fn check_conventions(node: &Node) -> Vec<String> {
    let mut warnings = Vec::new();
    let meta = &node.data.metadata;

    // entity_type should be a string
    if let Some(val) = meta.get(node_keys::ENTITY_TYPE) {
        if !val.is_string() {
            warnings.push("entity_type should be a string".into());
        }
    } else if node.kind.as_str() == "entity" {
        warnings.push("entity nodes should have an entity_type metadata key".into());
    }

    // aliases should be an array of strings
    if let Some(val) = meta.get(node_keys::ALIASES) {
        match val {
            Value::Array(arr) => {
                if !arr.iter().all(|v| v.is_string()) {
                    warnings.push("aliases should be an array of strings".into());
                }
            }
            _ => warnings.push("aliases should be an array of strings".into()),
        }
    }

    // parent_agent should be a string
    if let Some(val) = meta.get(node_keys::PARENT_AGENT) {
        if !val.is_string() {
            warnings.push("parent_agent should be a string".into());
        }
    }

    // task_id should be a string
    if let Some(val) = meta.get(node_keys::TASK_ID) {
        if !val.is_string() {
            warnings.push("task_id should be a string".into());
        }
    }

    // source_url should look like a URL
    if let Some(val) = meta.get(node_keys::SOURCE_URL) {
        match val {
            Value::String(url) => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    warnings.push("source_url should be an HTTP(S) URL".into());
                }
            }
            _ => warnings.push("source_url should be a string".into()),
        }
    }

    // content_type should be a string
    if let Some(val) = meta.get(node_keys::CONTENT_TYPE) {
        if !val.is_string() {
            warnings.push("content_type should be a string".into());
        }
    }

    // language should be a short string (ISO 639-1 codes are 2-3 chars)
    if let Some(val) = meta.get(node_keys::LANGUAGE) {
        match val {
            Value::String(lang) => {
                if lang.len() > 5 {
                    warnings
                        .push("language should be an ISO 639-1 code (e.g. \"en\", \"zh\")".into());
                }
            }
            _ => warnings.push("language should be a string".into()),
        }
    }

    // expires_reason should be a string
    if let Some(val) = meta.get(node_keys::EXPIRES_REASON) {
        if !val.is_string() {
            warnings.push("expires_reason should be a string".into());
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Node, NodeKind, Source};
    use serde_json::json;
    use std::collections::HashMap;

    fn make_node(kind: &str, metadata: HashMap<String, Value>) -> Node {
        let mut node = Node::new(
            NodeKind::new(kind).unwrap(),
            "Test node".into(),
            "Test body".into(),
            Source {
                agent: "test".into(),
                session: None,
                channel: None,
            },
            0.5,
        );
        node.data.metadata = metadata;
        node
    }

    #[test]
    fn test_no_warnings_for_valid_entity() {
        let mut meta = HashMap::new();
        meta.insert("entity_type".into(), json!("company"));
        meta.insert("aliases".into(), json!(["CompanyX", "company-x"]));
        let node = make_node("entity", meta);

        let warnings = check_conventions(&node);
        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_warning_entity_type_not_string() {
        let mut meta = HashMap::new();
        meta.insert("entity_type".into(), json!(42));
        let node = make_node("entity", meta);

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("entity_type should be a string")));
    }

    #[test]
    fn test_warning_missing_entity_type_on_entity_node() {
        let node = make_node("entity", HashMap::new());

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("entity nodes should have an entity_type")));
    }

    #[test]
    fn test_no_warning_missing_entity_type_on_fact_node() {
        let node = make_node("fact", HashMap::new());

        let warnings = check_conventions(&node);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_warning_aliases_not_array() {
        let mut meta = HashMap::new();
        meta.insert("aliases".into(), json!("single-string"));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("aliases should be an array")));
    }

    #[test]
    fn test_warning_aliases_contains_non_string() {
        let mut meta = HashMap::new();
        meta.insert("aliases".into(), json!(["valid", 42]));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("aliases should be an array of strings")));
    }

    #[test]
    fn test_warning_source_url_not_http() {
        let mut meta = HashMap::new();
        meta.insert("source_url".into(), json!("ftp://example.com/file"));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("source_url should be an HTTP(S) URL")));
    }

    #[test]
    fn test_valid_source_url_no_warning() {
        let mut meta = HashMap::new();
        meta.insert("source_url".into(), json!("https://example.com/doc"));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_warning_source_url_not_string() {
        let mut meta = HashMap::new();
        meta.insert("source_url".into(), json!(42));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings
            .iter()
            .any(|w| w.contains("source_url should be a string")));
    }

    #[test]
    fn test_warning_language_too_long() {
        let mut meta = HashMap::new();
        meta.insert("language".into(), json!("english-full-name"));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings.iter().any(|w| w.contains("ISO 639-1")));
    }

    #[test]
    fn test_valid_language_no_warning() {
        let mut meta = HashMap::new();
        meta.insert("language".into(), json!("en"));
        let node = make_node("fact", meta);

        let warnings = check_conventions(&node);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_entity_types_all_constant() {
        assert_eq!(entity_types::ALL.len(), 7);
        assert!(entity_types::ALL.contains(&"agent"));
        assert!(entity_types::ALL.contains(&"product"));
    }

    #[test]
    fn test_multiple_warnings_returned() {
        let mut meta = HashMap::new();
        meta.insert("entity_type".into(), json!(42));
        meta.insert("aliases".into(), json!("not-array"));
        meta.insert("source_url".into(), json!(false));
        let node = make_node("entity", meta);

        let warnings = check_conventions(&node);
        assert!(
            warnings.len() >= 3,
            "Expected at least 3 warnings, got {}",
            warnings.len()
        );
    }
}
