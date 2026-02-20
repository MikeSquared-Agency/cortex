use crate::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The JSON body stored in a prompt node.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptContent {
    pub slug: String,
    #[serde(rename = "type")]
    pub prompt_type: String, // persona | skill | constraint | template | meta
    pub sections: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub override_sections: HashMap<String, serde_json::Value>,
}

/// A fully resolved prompt with inheritance applied.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResolvedPrompt {
    pub slug: String,
    pub prompt_type: String,
    pub version: u32,
    pub branch: String,
    /// Fully merged sections (ancestor base + descendant overrides).
    pub content: HashMap<String, serde_json::Value>,
    /// This node's raw content before inheritance.
    pub raw_content: PromptContent,
    /// Ancestor slugs from deepest to shallowest, then this slug last.
    pub lineage: Vec<String>,
    /// Slugs of skill nodes with a `used_by` edge pointing to this prompt.
    pub skills: Vec<String>,
    pub node_id: NodeId,
    pub created_at: DateTime<Utc>,
}

/// Summary of a single version entry.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptVersionInfo {
    pub node_id: NodeId,
    pub slug: String,
    pub version: u32,
    pub branch: String,
    pub created_at: DateTime<Utc>,
    pub is_head: bool,
}

/// Summary of a prompt (HEAD of a slug+branch).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptInfo {
    pub slug: String,
    pub prompt_type: String,
    pub branch: String,
    pub version: u32,
    pub tags: Vec<String>,
    pub node_id: NodeId,
}
