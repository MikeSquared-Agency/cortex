//! Rust client for the Cortex graph memory engine.
//!
//! Thin wrapper over the tonic-generated gRPC client with ergonomic convenience methods.
//!
//! # Example
//! ```rust,no_run
//! use cortex_client::CortexClient;
//! use cortex_proto::cortex::v1::CreateNodeRequest;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut client = CortexClient::connect("http://localhost:9090").await?;
//!
//!     let node = client.create_node(CreateNodeRequest {
//!         kind: "decision".into(),
//!         title: "Use Rust for performance-critical paths".into(),
//!         body: "Go for I/O-bound, Rust for CPU-bound.".into(),
//!         importance: 0.8,
//!         ..Default::default()
//!     }).await?;
//!
//!     let results = client.search("language choices", 5).await?;
//!     let briefing = client.briefing("kai").await?;
//!
//!     println!("Node: {}", node.id);
//!     println!("Briefing:\n{}", briefing);
//!     Ok(())
//! }
//! ```
use cortex_proto::cortex::v1::{
    cortex_service_client::CortexServiceClient, BriefingRequest, CreateNodeRequest,
    GetNodeRequest, HybridResultEntry, HybridSearchRequest, NodeResponse, SearchResponse,
    SimilaritySearchRequest, SubgraphResponse, TraverseRequest,
};
use tonic::transport::Channel;

/// Re-export generated proto types for callers that need raw access.
pub use cortex_proto::cortex::v1 as proto;

/// A connected Cortex client.
///
/// Wraps the tonic gRPC client with ergonomic methods for common operations.
/// For full proto access use the [`proto`] re-export and call [`CortexClient::inner`].
pub struct CortexClient {
    inner: CortexServiceClient<Channel>,
}

impl CortexClient {
    /// Connect to a running Cortex server.
    ///
    /// `addr` should be a full URI, e.g. `"http://localhost:9090"`.
    pub async fn connect(addr: impl Into<String>) -> anyhow::Result<Self> {
        let channel = Channel::from_shared(addr.into())?.connect().await?;
        Ok(Self {
            inner: CortexServiceClient::new(channel),
        })
    }

    /// Expose the raw gRPC client for full proto access.
    pub fn inner(&mut self) -> &mut CortexServiceClient<Channel> {
        &mut self.inner
    }

    /// Create a node. Returns the stored [`NodeResponse`].
    pub async fn create_node(&mut self, req: CreateNodeRequest) -> anyhow::Result<NodeResponse> {
        let resp = self.inner.create_node(req).await?;
        Ok(resp.into_inner())
    }

    /// Get a node by ID. Returns `None` if not found.
    pub async fn get_node(&mut self, id: &str) -> anyhow::Result<Option<NodeResponse>> {
        match self.inner.get_node(GetNodeRequest { id: id.into() }).await {
            Ok(resp) => Ok(Some(resp.into_inner())),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Semantic similarity search. Returns scored result entries.
    pub async fn search(
        &mut self,
        query: &str,
        limit: u32,
    ) -> anyhow::Result<SearchResponse> {
        let resp = self
            .inner
            .similarity_search(SimilaritySearchRequest {
                query: query.into(),
                limit,
                ..Default::default()
            })
            .await?;
        Ok(resp.into_inner())
    }

    /// Hybrid search combining vector similarity with graph proximity.
    ///
    /// `anchor_ids` are node IDs that anchor the graph proximity component.
    /// Pass an empty `Vec` for pure hybrid mode with no anchors.
    pub async fn search_hybrid(
        &mut self,
        query: &str,
        anchor_ids: Vec<String>,
        limit: u32,
    ) -> anyhow::Result<Vec<HybridResultEntry>> {
        let resp = self
            .inner
            .hybrid_search(HybridSearchRequest {
                query: query.into(),
                anchor_ids,
                limit,
                ..Default::default()
            })
            .await?;
        Ok(resp.into_inner().results)
    }

    /// Generate a rendered context briefing for an agent. Returns markdown text.
    pub async fn briefing(&mut self, agent_id: &str) -> anyhow::Result<String> {
        let resp = self
            .inner
            .get_briefing(BriefingRequest {
                agent_id: agent_id.into(),
                ..Default::default()
            })
            .await?;
        Ok(resp.into_inner().rendered)
    }

    /// Graph traversal starting from `node_id` up to `depth` hops.
    pub async fn traverse(
        &mut self,
        node_id: &str,
        depth: u32,
    ) -> anyhow::Result<SubgraphResponse> {
        let resp = self
            .inner
            .traverse(TraverseRequest {
                start_ids: vec![node_id.into()],
                max_depth: depth,
                ..Default::default()
            })
            .await?;
        Ok(resp.into_inner())
    }
}
