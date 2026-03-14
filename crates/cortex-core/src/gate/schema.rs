//! Optional per-kind schema validation for node metadata.
//!
//! When schemas are defined (in `cortex.toml`), nodes of those kinds have their
//! `metadata` fields validated at write time. Kinds without schemas pass freely.

use crate::Node;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The type of a metadata field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Array,
}

/// Schema for a single metadata field.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FieldSchema {
    /// Expected type of the field value.
    #[serde(rename = "type")]
    pub field_type: Option<FieldType>,
    /// Minimum value (for Number fields).
    pub min: Option<f64>,
    /// Maximum value (for Number fields).
    pub max: Option<f64>,
    /// Allowed values (enum-like constraint).
    pub allowed_values: Option<Vec<String>>,
}

/// Schema definition for a specific node kind.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KindSchema {
    /// Fields that must be present in node metadata.
    pub required_fields: Vec<String>,
    /// Per-field type and constraint definitions.
    pub fields: HashMap<String, FieldSchema>,
}

/// A single schema violation.
#[derive(Debug, Clone)]
pub struct SchemaViolation {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for SchemaViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Validates node metadata against per-kind schemas.
#[derive(Debug, Clone, Default)]
pub struct SchemaValidator {
    schemas: HashMap<String, KindSchema>,
}

impl SchemaValidator {
    /// Create a new validator with the given schemas (kind -> schema).
    pub fn new(schemas: HashMap<String, KindSchema>) -> Self {
        Self { schemas }
    }

    /// Create an empty validator that passes everything.
    pub fn empty() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Returns true if any schemas are configured.
    pub fn has_schemas(&self) -> bool {
        !self.schemas.is_empty()
    }

    /// Validate a node against its kind's schema.
    ///
    /// Returns `Ok(())` if:
    /// - No schema is registered for this node's kind (backward compatible)
    /// - All schema constraints are satisfied
    ///
    /// Returns `Err(violations)` if any constraints fail.
    pub fn validate(&self, node: &Node) -> Result<(), Vec<SchemaViolation>> {
        let kind_str = node.kind.as_str();
        let schema = match self.schemas.get(kind_str) {
            Some(s) => s,
            None => return Ok(()), // No schema -> pass
        };

        let mut violations = Vec::new();
        let metadata = &node.data.metadata;

        // Check required fields
        for field_name in &schema.required_fields {
            if !metadata.contains_key(field_name) {
                violations.push(SchemaViolation {
                    field: field_name.clone(),
                    message: "required field missing".to_string(),
                });
            }
        }

        // Check field constraints
        for (field_name, field_schema) in &schema.fields {
            if let Some(value) = metadata.get(field_name) {
                // Type check
                if let Some(ref expected_type) = field_schema.field_type {
                    let type_ok = match expected_type {
                        FieldType::String => value.is_string(),
                        FieldType::Number => value.is_number(),
                        FieldType::Boolean => value.is_boolean(),
                        FieldType::Array => value.is_array(),
                    };
                    if !type_ok {
                        violations.push(SchemaViolation {
                            field: field_name.clone(),
                            message: format!(
                                "expected type {:?}, got {}",
                                expected_type,
                                json_type_name(value)
                            ),
                        });
                        continue; // Skip further checks if type is wrong
                    }
                }

                // Numeric range checks
                if let Some(num) = value.as_f64() {
                    if let Some(min) = field_schema.min {
                        if num < min {
                            violations.push(SchemaViolation {
                                field: field_name.clone(),
                                message: format!("value {} is below minimum {}", num, min),
                            });
                        }
                    }
                    if let Some(max) = field_schema.max {
                        if num > max {
                            violations.push(SchemaViolation {
                                field: field_name.clone(),
                                message: format!("value {} exceeds maximum {}", num, max),
                            });
                        }
                    }
                }

                // Allowed values check
                if let Some(ref allowed) = field_schema.allowed_values {
                    let val_str = match value.as_str() {
                        Some(s) => s.to_string(),
                        None => value.to_string(),
                    };
                    if !allowed.contains(&val_str) {
                        violations.push(SchemaViolation {
                            field: field_name.clone(),
                            message: format!(
                                "value {:?} not in allowed values: {:?}",
                                val_str, allowed
                            ),
                        });
                    }
                }
            }
            // Note: if a field is defined in field_schemas but not in metadata AND
            // not in required_fields, that's fine -- it's optional.
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, NodeKind, Source};
    use serde_json::json;

    fn make_node_with_metadata(kind: &str, metadata: serde_json::Value) -> Node {
        let mut node = Node::new(
            NodeKind::new(kind).unwrap(),
            "Test node title here".to_string(),
            "Test node body content for testing".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        );
        if let serde_json::Value::Object(map) = metadata {
            node.data.metadata = map.into_iter().collect();
        }
        node
    }

    fn make_decision_schema() -> HashMap<String, KindSchema> {
        let mut fields = HashMap::new();
        fields.insert(
            "rationale".to_string(),
            FieldSchema {
                field_type: Some(FieldType::String),
                ..Default::default()
            },
        );
        fields.insert(
            "priority".to_string(),
            FieldSchema {
                field_type: Some(FieldType::Number),
                min: Some(1.0),
                max: Some(5.0),
                ..Default::default()
            },
        );

        let mut schemas = HashMap::new();
        schemas.insert(
            "decision".to_string(),
            KindSchema {
                required_fields: vec!["rationale".to_string()],
                fields,
            },
        );
        schemas
    }

    #[test]
    fn test_no_schema_passes() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata("fact", json!({}));
        assert!(validator.validate(&node).is_ok());
    }

    #[test]
    fn test_empty_validator_passes() {
        let validator = SchemaValidator::empty();
        let node = make_node_with_metadata("decision", json!({}));
        assert!(validator.validate(&node).is_ok());
    }

    #[test]
    fn test_required_field_missing() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata("decision", json!({}));
        let err = validator.validate(&node).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "rationale");
        assert!(err[0].message.contains("required"));
    }

    #[test]
    fn test_required_field_present() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata(
            "decision",
            json!({"rationale": "We chose redb for performance"}),
        );
        assert!(validator.validate(&node).is_ok());
    }

    #[test]
    fn test_type_mismatch() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata(
            "decision",
            json!({"rationale": "test", "priority": "high"}),
        );
        let err = validator.validate(&node).unwrap_err();
        assert!(err
            .iter()
            .any(|v| v.field == "priority" && v.message.contains("expected type")));
    }

    #[test]
    fn test_numeric_below_min() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata(
            "decision",
            json!({"rationale": "test", "priority": 0}),
        );
        let err = validator.validate(&node).unwrap_err();
        assert!(err
            .iter()
            .any(|v| v.field == "priority" && v.message.contains("below minimum")));
    }

    #[test]
    fn test_numeric_above_max() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata(
            "decision",
            json!({"rationale": "test", "priority": 10}),
        );
        let err = validator.validate(&node).unwrap_err();
        assert!(err
            .iter()
            .any(|v| v.field == "priority" && v.message.contains("exceeds maximum")));
    }

    #[test]
    fn test_valid_node_passes() {
        let validator = SchemaValidator::new(make_decision_schema());
        let node = make_node_with_metadata(
            "decision",
            json!({"rationale": "We chose redb", "priority": 3}),
        );
        assert!(validator.validate(&node).is_ok());
    }

    #[test]
    fn test_allowed_values() {
        let mut fields = HashMap::new();
        fields.insert(
            "status".to_string(),
            FieldSchema {
                field_type: Some(FieldType::String),
                allowed_values: Some(vec![
                    "open".to_string(),
                    "closed".to_string(),
                    "pending".to_string(),
                ]),
                ..Default::default()
            },
        );
        let mut schemas = HashMap::new();
        schemas.insert(
            "fact".to_string(),
            KindSchema {
                required_fields: vec![],
                fields,
            },
        );
        let validator = SchemaValidator::new(schemas);

        // Valid
        let node = make_node_with_metadata("fact", json!({"status": "open"}));
        assert!(validator.validate(&node).is_ok());

        // Invalid
        let node = make_node_with_metadata("fact", json!({"status": "invalid"}));
        assert!(validator.validate(&node).is_err());
    }

    #[test]
    fn test_has_schemas() {
        let empty = SchemaValidator::empty();
        assert!(!empty.has_schemas());

        let with = SchemaValidator::new(make_decision_schema());
        assert!(with.has_schemas());
    }
}
