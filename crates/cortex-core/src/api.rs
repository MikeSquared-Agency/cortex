use crate::{
    CortexError, Result,
    Node, Edge, NodeId, NodeKind, Source,
    NodeFilter,
    RedbStorage, FastEmbedService, HnswIndex,
    GraphEngineImpl, GraphEngine, Storage, VectorIndex, EmbeddingService,
};
use crate::vector::embedding_input;
use crate::linker::AutoLinkerConfig;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// Config for embedded library mode.
#[derive(Debug, Clone)]
pub struct LibraryConfig {
    /// Embedding model identifier. Default: "BAAI/bge-small-en-v1.5"
    pub embedding_model: String,
    /// Auto-linker config. Used if you call `run_auto_linker()`.
    pub auto_linker: AutoLinkerConfig,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            embedding_model: "BAAI/bge-small-en-v1.5".into(),
            auto_linker: AutoLinkerConfig::new(),
        }
    }
}

/// High-level, embedded Cortex API. No server required.
///
/// # Example
/// ```rust,no_run
/// use cortex_core::{Cortex, LibraryConfig};
///
/// let cortex = Cortex::open("./memory.redb", LibraryConfig::default()).unwrap();
/// cortex.store(Cortex::fact("The API uses JWT auth", 0.7)).unwrap();
/// let results = cortex.search("authentication", 5).unwrap();
/// ```
pub struct Cortex {
    storage: Arc<RedbStorage>,
    embedding: Arc<FastEmbedService>,
    index: Arc<RwLock<HnswIndex>>,
    graph_engine: Arc<GraphEngineImpl<RedbStorage>>,
    #[allow(dead_code)]
    config: LibraryConfig,
}

impl Cortex {
    /// Open (or create) a Cortex database at the given path.
    pub fn open(path: impl AsRef<Path>, config: LibraryConfig) -> Result<Self> {
        let storage = Arc::new(RedbStorage::open(path.as_ref())?);

        let embedding = Arc::new(Self::create_embedding_service(&config.embedding_model)?);

        // Build HNSW index from existing nodes
        let index = {
            let mut idx = HnswIndex::new(embedding.dimension());
            let nodes = storage.list_nodes(NodeFilter::new())?;
            let mut any = false;
            for node in &nodes {
                if let Some(emb) = &node.embedding {
                    idx.insert(node.id, emb)?;
                    any = true;
                }
            }
            if any {
                idx.rebuild()?;
            }
            Arc::new(RwLock::new(idx))
        };

        let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

        Ok(Self { storage, embedding, index, graph_engine, config })
    }

    fn create_embedding_service(model: &str) -> Result<FastEmbedService> {
        use fastembed::EmbeddingModel;
        match model {
            "BAAI/bge-base-en-v1.5" => FastEmbedService::with_model(EmbeddingModel::BGEBaseENV15),
            "BAAI/bge-large-en-v1.5" => FastEmbedService::with_model(EmbeddingModel::BGELargeENV15),
            _ => FastEmbedService::new(),
        }
    }

    /// Store a node, generating its embedding automatically.
    pub fn store(&self, mut node: Node) -> Result<NodeId> {
        if node.embedding.is_none() {
            let text = embedding_input(&node);
            node.embedding = Some(self.embedding.embed(&text)?);
        }
        let id = node.id;
        let emb = node.embedding.clone().unwrap();
        self.storage.put_node(&node)?;
        self.index.write()
            .map_err(|_| CortexError::Validation("Vector index lock poisoned".into()))?
            .insert(id, &emb)?;
        Ok(id)
    }

    /// Semantic similarity search. Returns nodes ranked by score.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<(f32, Node)>> {
        let query_emb = self.embedding.embed(query)?;
        let results = self.index.read()
            .map_err(|_| CortexError::Validation("Vector index lock poisoned".into()))?
            .search(&query_emb, limit, None)?;
        let mut out = Vec::new();
        for r in results {
            if let Some(node) = self.storage.get_node(r.node_id)? {
                out.push((r.score, node));
            }
        }
        Ok(out)
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Result<Option<Node>> {
        self.storage.get_node(id)
    }

    /// List nodes with optional filter.
    pub fn list_nodes(&self, filter: NodeFilter) -> Result<Vec<Node>> {
        self.storage.list_nodes(filter)
    }

    /// Create an edge between two nodes.
    pub fn create_edge(&self, edge: Edge) -> Result<()> {
        self.storage.put_edge(&edge)
    }

    /// Graph traversal from a node (returns neighborhood).
    pub fn traverse(&self, from: NodeId, depth: u32) -> Result<crate::graph::Subgraph> {
        self.graph_engine.neighborhood(from, depth)
    }

    /// Hybrid search (vector + graph proximity). Not yet implemented.
    pub fn search_hybrid(&self, _query: &str, _limit: usize) -> Result<Vec<(f32, Node)>> {
        Err(CortexError::Validation(
            "search_hybrid not yet implemented in library mode".into(),
        ))
    }

    /// Generate a briefing string for an agent. Not yet implemented in library mode.
    pub fn briefing(&self, _agent_id: &str) -> Result<String> {
        Err(CortexError::Validation(
            "briefing not yet implemented in library mode".into(),
        ))
    }

    // --- Convenience node constructors ---

    fn make_node(kind: &str, title: &str, body: &str, importance: f32) -> Node {
        Node::new(
            NodeKind::new(kind).unwrap(),
            title.into(),
            body.into(),
            Source { agent: "library".into(), session: None, channel: None },
            importance,
        )
    }

    pub fn fact(title: &str, importance: f32) -> Node {
        Self::make_node("fact", title, title, importance)
    }

    pub fn decision(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("decision", title, body, importance)
    }

    pub fn event(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("event", title, body, importance)
    }

    pub fn goal(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("goal", title, body, importance)
    }

    pub fn observation(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("observation", title, body, importance)
    }

    pub fn pattern(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("pattern", title, body, importance)
    }

    pub fn preference(title: &str, body: &str, importance: f32) -> Node {
        Self::make_node("preference", title, body, importance)
    }
}
