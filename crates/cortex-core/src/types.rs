use crate::error::{CortexError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Type alias for node identifiers
pub type NodeId = Uuid;

/// Serde default for `last_accessed_at`: Unix epoch, meaning "never accessed".
/// Existing nodes deserialized without this field will appear maximally stale,
/// flooring at `min_factor` in the score decay formula.
fn default_last_accessed_at() -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH
}

/// Type alias for edge identifiers
pub type EdgeId = Uuid;

/// Type alias for embedding vectors
pub type Embedding = Vec<f32>;

/// A knowledge node in the graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    /// Unique identifier. UUIDv7 for time-sortability.
    pub id: NodeId,

    /// What kind of knowledge this represents.
    pub kind: NodeKind,

    /// The actual content. Structured but flexible.
    pub data: NodeData,

    /// Optional pre-computed embedding vector.
    /// None until the vector layer processes this node.
    pub embedding: Option<Embedding>,

    /// Which agent or process created this node.
    pub source: Source,

    /// Importance score (0.0 - 1.0). Affects retrieval ranking
    /// and decay rate. Higher importance decays slower.
    pub importance: f32,

    /// How many times this node has been accessed/referenced.
    /// Used for reinforcement — frequently accessed nodes
    /// resist decay.
    pub access_count: u64,

    /// Last time this node was returned in a search result or included in a
    /// briefing. Distinct from `updated_at` (which changes on content edits).
    /// Used by query-time score decay to compute days_idle.
    /// Defaults to the Unix epoch for nodes created before this field existed.
    #[serde(default = "default_last_accessed_at")]
    pub last_accessed_at: DateTime<Utc>,

    /// When this knowledge was created.
    pub created_at: DateTime<Utc>,

    /// Last time this node was modified or accessed.
    pub updated_at: DateTime<Utc>,

    /// Soft delete. Nodes are never physically removed,
    /// only tombstoned. Allows undo and audit.
    pub deleted: bool,

    /// When this fact became true in the real world.
    /// None = true since created_at.
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,

    /// When this fact stopped being true in the real world.
    /// None = still true.
    /// The node remains in the graph for historical queries.
    /// Briefings and search should exclude expired facts by default.
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,

    /// When to garbage-collect this node.
    /// None = permanent.
    /// Distinct from valid_until: valid_until is epistemic ("stopped being true"),
    /// expires_at is lifecycle ("delete from graph").
    /// The retention engine sweeps nodes past their expires_at.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,

    /// Which embedding model generated the vector in `embedding`.
    /// None = unknown (existing nodes, or nodes without embeddings).
    #[serde(default)]
    pub embedding_model: Option<String>,
}

/// A node kind identifier. Lowercase alphanumeric + hyphens only.
/// Validated on construction.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeKind(String);

impl NodeKind {
    pub fn new(kind: &str) -> Result<Self> {
        if kind.is_empty() {
            return Err(CortexError::Validation("NodeKind cannot be empty".into()));
        }
        if !kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(CortexError::Validation(format!(
                "NodeKind '{}' must be lowercase alphanumeric + hyphens only",
                kind
            )));
        }
        Ok(NodeKind(kind.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Debug produces PascalCase for API compatibility: "fact" → "Fact"
impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut chars = self.0.chars();
        match chars.next() {
            None => write!(f, ""),
            Some(c) => write!(f, "{}{}", c.to_uppercase(), chars.as_str()),
        }
    }
}

impl TryFrom<&str> for NodeKind {
    type Error = CortexError;
    fn try_from(s: &str) -> Result<Self> {
        NodeKind::new(s)
    }
}

impl TryFrom<String> for NodeKind {
    type Error = CortexError;
    fn try_from(s: String) -> Result<Self> {
        NodeKind::new(&s)
    }
}

/// Node content structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeData {
    /// Human-readable title/summary. Required.
    /// Max 256 chars. Used for display and quick scanning.
    pub title: String,

    /// Full content. Required.
    /// No max length but embedding quality degrades past ~2000 chars.
    pub body: String,

    /// Arbitrary key-value metadata. Optional.
    /// Use for: source URLs, file paths, commit SHAs,
    /// agent IDs, task IDs, timestamps of the thing described.
    pub metadata: HashMap<String, Value>,

    /// Tags for lightweight categorisation.
    /// Not a replacement for NodeKind — tags are ad-hoc,
    /// kinds are structural.
    pub tags: Vec<String>,
}

/// A relationship between two nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    /// Unique identifier. UUIDv7.
    pub id: EdgeId,

    /// Source node.
    pub from: NodeId,

    /// Target node.
    pub to: NodeId,

    /// What this relationship means.
    pub relation: Relation,

    /// Strength of the relationship (0.0 - 1.0).
    /// Auto-created edges start at the similarity score.
    /// Manual edges start at 1.0.
    /// Decays over time unless reinforced by access.
    pub weight: f32,

    /// How this edge was created.
    pub provenance: EdgeProvenance,

    /// When this edge was created.
    pub created_at: DateTime<Utc>,

    /// Last time weight was updated (access or decay).
    pub updated_at: DateTime<Utc>,

    /// Extensible key-value data for this edge.
    /// Use cases: shared entity name linking two nodes,
    /// similarity context, rule parameters, external references.
    /// Default: empty map (backward-compatible with existing edges).
    /// Uses String values (not serde_json::Value) for bincode compatibility.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// A relation type identifier. Lowercase alphanumeric + underscores only.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Relation(String);

impl Relation {
    pub fn new(relation: &str) -> Result<Self> {
        if relation.is_empty() {
            return Err(CortexError::Validation("Relation cannot be empty".into()));
        }
        if !relation
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(CortexError::Validation(format!(
                "Relation '{}' must be lowercase alphanumeric + underscores only",
                relation
            )));
        }
        Ok(Relation(relation.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Debug produces PascalCase for API compatibility: "related_to" → "RelatedTo"
impl std::fmt::Debug for Relation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pascal: String = self
            .0
            .split('_')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect();
        write!(f, "{}", pascal)
    }
}

impl TryFrom<&str> for Relation {
    type Error = CortexError;
    fn try_from(s: &str) -> Result<Self> {
        Relation::new(s)
    }
}

impl TryFrom<String> for Relation {
    type Error = CortexError;
    fn try_from(s: String) -> Result<Self> {
        Relation::new(&s)
    }
}

/// How an edge was created
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EdgeProvenance {
    /// Created explicitly by an agent or human.
    Manual { created_by: String },

    /// Created automatically by the auto-linker
    /// based on embedding similarity.
    AutoSimilarity { score: f32 },

    /// Created automatically by the auto-linker
    /// based on structural rules (e.g., same tags,
    /// same source, temporal proximity).
    AutoStructural { rule: String },

    /// Created automatically by the auto-linker
    /// when detecting contradictions.
    AutoContradiction { reason: String },

    /// Created automatically by the auto-linker
    /// during deduplication.
    AutoDedup { similarity: f32 },

    /// Imported from an external source (Alexandria migration).
    Imported { source: String },

    /// Extensible provenance for linking mechanisms not covered
    /// by the built-in variants. Forward-compatible: older code
    /// that doesn't know about a specific custom kind can still
    /// deserialise and display it.
    Custom {
        /// Machine-readable type identifier.
        /// Examples: "entity_resolution", "user_plugin", "external_sync"
        kind: String,
        /// Arbitrary structured detail.
        detail: HashMap<String, String>,
    },
}

/// Source of a node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    /// Which agent created this. "kai", "dutybound", "worker-123", "human".
    pub agent: String,

    /// Which session/conversation. Optional.
    pub session: Option<String>,

    /// Which channel. Optional. "whatsapp", "slack", "terminal".
    pub channel: Option<String>,
}

impl Node {
    /// Create a new node with the given parameters
    pub fn new(
        kind: NodeKind,
        title: String,
        body: String,
        source: Source,
        importance: f32,
    ) -> Self {
        let now = Utc::now();
        Node {
            id: Uuid::now_v7(),
            kind,
            data: NodeData {
                title,
                body,
                metadata: HashMap::new(),
                tags: Vec::new(),
            },
            embedding: None,
            source,
            importance: importance.clamp(0.0, 1.0),
            access_count: 0,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
            deleted: false,
            valid_from: None,
            valid_until: None,
            expires_at: None,
            embedding_model: None,
        }
    }

    pub fn with_valid_from(mut self, t: DateTime<Utc>) -> Self {
        self.valid_from = Some(t);
        self
    }

    pub fn with_valid_until(mut self, t: DateTime<Utc>) -> Self {
        self.valid_until = Some(t);
        self
    }

    pub fn with_expires_at(mut self, t: DateTime<Utc>) -> Self {
        self.expires_at = Some(t);
        self
    }

    pub fn with_embedding_model(mut self, model: String) -> Self {
        self.embedding_model = Some(model);
        self
    }

    /// Validate the node according to the rules in the spec
    pub fn validate(&self) -> std::result::Result<(), String> {
        // Title length check
        if self.data.title.chars().count() > 256 {
            return Err("Title exceeds 256 characters".to_string());
        }

        // Importance range check
        if !(0.0..=1.0).contains(&self.importance) {
            return Err(format!(
                "Importance {} out of range [0.0, 1.0]",
                self.importance
            ));
        }

        // Tags validation
        if self.data.tags.len() > 32 {
            return Err("More than 32 tags".to_string());
        }

        // valid_from must be before valid_until when both are set
        if let (Some(from), Some(until)) = (self.valid_from, self.valid_until) {
            if from >= until {
                return Err("valid_from must be before valid_until".to_string());
            }
        }

        for tag in &self.data.tags {
            if tag.chars().count() > 64 {
                return Err(format!("Tag '{}' exceeds 64 characters", tag));
            }
            if !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(format!(
                    "Tag '{}' contains invalid characters (only alphanumeric and hyphens allowed)",
                    tag
                ));
            }
            if tag != &tag.to_lowercase() {
                return Err(format!("Tag '{}' must be lowercase", tag));
            }
        }

        Ok(())
    }

    /// Increment access count and update last-accessed timestamp.
    /// Called when this node is returned in a search result or included in a briefing.
    pub fn record_access(&mut self) {
        let now = Utc::now();
        self.access_count += 1;
        self.last_accessed_at = now;
        self.updated_at = now;
    }
}

impl Edge {
    /// Create a new edge with the given parameters
    pub fn new(
        from: NodeId,
        to: NodeId,
        relation: Relation,
        weight: f32,
        provenance: EdgeProvenance,
    ) -> Self {
        let now = Utc::now();
        Edge {
            id: Uuid::now_v7(),
            from,
            to,
            relation,
            weight: weight.clamp(0.0, 1.0),
            provenance,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_metadata_entry(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Validate the edge according to the rules in the spec
    pub fn validate(&self) -> std::result::Result<(), String> {
        // Self-edge check
        if self.from == self.to {
            return Err("Self-edges are not allowed".to_string());
        }

        // Weight range check
        if !(0.0..=1.0).contains(&self.weight) {
            return Err(format!("Weight {} out of range [0.0, 1.0]", self.weight));
        }

        Ok(())
    }

    /// Update weight and timestamp
    pub fn update_weight(&mut self, new_weight: f32) {
        self.weight = new_weight.clamp(0.0, 1.0);
        self.updated_at = Utc::now();
    }
}
