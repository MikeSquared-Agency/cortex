use crate::grpc::conversions::*;
use cortex_core::*;
// cortex_core::* imports a 1-arg `Result<T>` alias; re-import std's 2-arg form
// so that tonic handler return types like `Result<Response<T>, Status>` resolve correctly.
use std::result::Result;
use cortex_proto::cortex_service_server::CortexService;
use cortex_proto::*;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::Instant;
use tonic::{Request, Response, Status};

pub struct CortexServiceImpl {
    storage: Arc<RedbStorage>,
    graph_engine: Arc<GraphEngineImpl<RedbStorage>>,
    vector_index: Arc<StdRwLock<HnswIndex>>,
    embedding_service: Arc<FastEmbedService>,
    auto_linker: Arc<StdRwLock<AutoLinker<RedbStorage, FastEmbedService, HnswIndex, GraphEngineImpl<RedbStorage>>>>,
    start_time: Instant,
}

impl CortexServiceImpl {
    pub fn new(
        storage: Arc<RedbStorage>,
        graph_engine: Arc<GraphEngineImpl<RedbStorage>>,
        vector_index: Arc<StdRwLock<HnswIndex>>,
        embedding_service: Arc<FastEmbedService>,
        auto_linker: Arc<StdRwLock<AutoLinker<RedbStorage, FastEmbedService, HnswIndex, GraphEngineImpl<RedbStorage>>>>,
    ) -> Self {
        Self {
            storage,
            graph_engine,
            vector_index,
            embedding_service,
            auto_linker,
            start_time: Instant::now(),
        }
    }

    fn get_edge_count(&self, node_id: NodeId) -> usize {
        let outgoing = self.storage.edges_from(node_id).unwrap_or_default();
        let incoming = self.storage.edges_to(node_id).unwrap_or_default();
        outgoing.len() + incoming.len()
    }
}

#[tonic::async_trait]
impl CortexService for CortexServiceImpl {
    async fn create_node(
        &self,
        request: Request<CreateNodeRequest>,
    ) -> Result<Response<NodeResponse>, Status> {
        let req = request.into_inner();

        let kind = parse_node_kind(&req.kind).map_err(|e| Status::invalid_argument(e.to_string()))?;

        let source = Source {
            agent: req.source_agent,
            session: req.source_session,
            channel: req.source_channel,
        };

        let mut node = Node::new(
            kind,
            req.title,
            req.body,
            source,
            req.importance,
        );

        // Proto metadata is HashMap<String, String>; node metadata is HashMap<String, Value>
        node.data.metadata = req.metadata
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        node.data.tags = req.tags;

        // Generate embedding
        let text = embedding_input(&node);
        let embedding = self.embedding_service
            .embed(&text)
            .map_err(|e| Status::internal(e.to_string()))?;
        node.embedding = Some(embedding.clone());

        // Store node
        self.storage
            .put_node(&node)
            .map_err(|e| Status::internal(e.to_string()))?;

        // Index embedding
        {
            let mut index = self.vector_index.write().unwrap();
            index.insert(node.id, &embedding)
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        let edge_count = self.get_edge_count(node.id);
        Ok(Response::new(node_to_response(&node, edge_count)))
    }

    async fn get_node(
        &self,
        request: Request<GetNodeRequest>,
    ) -> Result<Response<NodeResponse>, Status> {
        let req = request.into_inner();
        let node_id = req.id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        let node = self.storage
            .get_node(node_id)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Node not found"))?;

        let edge_count = self.get_edge_count(node.id);
        Ok(Response::new(node_to_response(&node, edge_count)))
    }

    async fn update_node(
        &self,
        request: Request<UpdateNodeRequest>,
    ) -> Result<Response<NodeResponse>, Status> {
        let req = request.into_inner();
        let node_id = req.id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        let mut node = self.storage
            .get_node(node_id)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Node not found"))?;

        // Update fields
        if let Some(title) = req.title {
            node.data.title = title;
        }
        if let Some(body) = req.body {
            node.data.body = body;
        }
        if !req.metadata.is_empty() {
            node.data.metadata = req.metadata
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect();
        }
        if !req.tags.is_empty() {
            node.data.tags = req.tags;
        }
        if let Some(importance) = req.importance {
            node.importance = importance;
        }

        // Re-generate embedding
        let text = embedding_input(&node);
        let embedding = self.embedding_service
            .embed(&text)
            .map_err(|e| Status::internal(e.to_string()))?;
        node.embedding = Some(embedding.clone());
        node.updated_at = chrono::Utc::now();

        // Update storage
        self.storage
            .put_node(&node)
            .map_err(|e| Status::internal(e.to_string()))?;

        // Update index
        {
            let mut index = self.vector_index.write().unwrap();
            index.insert(node.id, &embedding)
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        let edge_count = self.get_edge_count(node.id);
        Ok(Response::new(node_to_response(&node, edge_count)))
    }

    async fn delete_node(
        &self,
        request: Request<DeleteNodeRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let node_id = req.id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        self.storage
            .delete_node(node_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(DeleteResponse { success: true }))
    }

    async fn list_nodes(
        &self,
        request: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let req = request.into_inner();

        let mut filter = NodeFilter::new();

        if !req.kind_filter.is_empty() {
            let kinds: Result<Vec<_>, _> = req.kind_filter.iter()
                .map(|s| parse_node_kind(s))
                .collect();
            filter = filter.with_kinds(kinds.map_err(|e| Status::invalid_argument(e.to_string()))?);
        }

        if !req.tag_filter.is_empty() {
            filter = filter.with_tags(req.tag_filter);
        }

        if !req.source_agent.is_empty() {
            filter = filter.with_source_agent(req.source_agent);
        }

        if req.min_importance > 0.0 {
            filter = filter.with_min_importance(req.min_importance);
        }

        if req.limit > 0 {
            filter = filter.with_limit(req.limit as usize);
        }

        if req.offset > 0 {
            filter = filter.with_offset(req.offset as usize);
        }

        let nodes = self.storage
            .list_nodes(filter.clone())
            .map_err(|e| Status::internal(e.to_string()))?;

        let total_count = self.storage
            .count_nodes(filter)
            .map_err(|e| Status::internal(e.to_string()))?;

        let node_responses: Vec<_> = nodes
            .iter()
            .map(|n| {
                let edge_count = self.get_edge_count(n.id);
                node_to_response(n, edge_count)
            })
            .collect();

        Ok(Response::new(ListNodesResponse {
            nodes: node_responses,
            total_count,
        }))
    }

    async fn create_edge(
        &self,
        request: Request<CreateEdgeRequest>,
    ) -> Result<Response<EdgeResponse>, Status> {
        let req = request.into_inner();

        let from_id = req.from_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid from_id: {}", e)))?;
        let to_id = req.to_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid to_id: {}", e)))?;

        let relation = parse_relation(&req.relation)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let edge = Edge::new(
            from_id,
            to_id,
            relation,
            req.weight,
            EdgeProvenance::Manual {
                created_by: "grpc_api".to_string(),
            },
        );

        self.storage
            .put_edge(&edge)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(edge_to_response(&edge)))
    }

    async fn get_edges(
        &self,
        request: Request<GetEdgesRequest>,
    ) -> Result<Response<GetEdgesResponse>, Status> {
        let req = request.into_inner();

        let node_id = req.node_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        let direction = parse_direction(&req.direction);

        let mut edges = Vec::new();

        match direction {
            TraversalDirection::Outgoing => {
                edges = self.storage.edges_from(node_id)
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
            TraversalDirection::Incoming => {
                edges = self.storage.edges_to(node_id)
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
            TraversalDirection::Both => {
                let outgoing = self.storage.edges_from(node_id)
                    .map_err(|e| Status::internal(e.to_string()))?;
                let incoming = self.storage.edges_to(node_id)
                    .map_err(|e| Status::internal(e.to_string()))?;
                edges.extend(outgoing);
                edges.extend(incoming);
            }
        }

        let edge_responses: Vec<_> = edges.iter().map(edge_to_response).collect();

        Ok(Response::new(GetEdgesResponse {
            edges: edge_responses,
        }))
    }

    async fn delete_edge(
        &self,
        request: Request<DeleteEdgeRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        let edge_id = req.id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        self.storage
            .delete_edge(edge_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(DeleteResponse { success: true }))
    }

    async fn traverse(
        &self,
        request: Request<TraverseRequest>,
    ) -> Result<Response<SubgraphResponse>, Status> {
        let req = request.into_inner();

        let start: Result<Vec<_>, _> = req.start_ids.iter()
            .map(|s| s.parse::<uuid::Uuid>())
            .collect();
        let start = start.map_err(|e| Status::invalid_argument(format!("Invalid start_ids: {}", e)))?;

        let direction = parse_direction(&req.direction);
        let strategy = parse_strategy(&req.strategy);

        let mut traverse_req = TraversalRequest {
            start,
            max_depth: if req.max_depth > 0 { Some(req.max_depth) } else { None },
            direction,
            strategy,
            limit: if req.limit > 0 { Some(req.limit as usize) } else { None },
            ..Default::default()
        };

        if !req.relation_filter.is_empty() {
            let relations: Result<Vec<_>, _> = req.relation_filter.iter()
                .map(|s| parse_relation(s))
                .collect();
            traverse_req.relation_filter = Some(relations.map_err(|e| Status::invalid_argument(e.to_string()))?);
        }

        if !req.kind_filter.is_empty() {
            let kinds: Result<Vec<_>, _> = req.kind_filter.iter()
                .map(|s| parse_node_kind(s))
                .collect();
            traverse_req.kind_filter = Some(kinds.map_err(|e| Status::invalid_argument(e.to_string()))?);
        }

        if req.min_weight > 0.0 {
            traverse_req.min_weight = Some(req.min_weight);
        }

        let subgraph = self.graph_engine
            .traverse(traverse_req)
            .map_err(|e| Status::internal(e.to_string()))?;

        let nodes: Vec<_> = subgraph.nodes.values()
            .map(|n| {
                let edge_count = self.get_edge_count(n.id);
                node_to_response(n, edge_count)
            })
            .collect();

        let edges: Vec<_> = subgraph.edges.iter().map(edge_to_response).collect();

        let depths: std::collections::HashMap<String, u32> = subgraph.depths
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        Ok(Response::new(SubgraphResponse {
            nodes,
            edges,
            depths,
            visited_count: subgraph.visited_count as u32,
            truncated: subgraph.truncated,
        }))
    }

    async fn find_paths(
        &self,
        request: Request<FindPathsRequest>,
    ) -> Result<Response<PathsResponse>, Status> {
        let req = request.into_inner();

        let from = req.from_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid from_id: {}", e)))?;
        let to = req.to_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid to_id: {}", e)))?;

        let path_req = PathRequest {
            from,
            to,
            max_paths: if req.max_paths > 0 { req.max_paths as usize } else { 1 },
            max_length: if req.max_depth > 0 { Some(req.max_depth) } else { None },
            ..Default::default()
        };

        let paths = self.graph_engine
            .find_paths(path_req)
            .map_err(|e| Status::internal(e.to_string()))?;

        let path_entries: Vec<_> = paths.paths.iter()
            .map(|p| PathEntry {
                node_ids: p.nodes.iter().map(|id| id.to_string()).collect(),
                total_weight: p.total_weight,
                length: p.nodes.len() as u32,
            })
            .collect();

        Ok(Response::new(PathsResponse {
            paths: path_entries,
        }))
    }

    async fn neighborhood(
        &self,
        request: Request<NeighborhoodRequest>,
    ) -> Result<Response<SubgraphResponse>, Status> {
        let req = request.into_inner();

        let node_id = req.node_id.parse::<uuid::Uuid>()
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        let _direction = parse_direction(&req.direction);
        let depth = if req.depth > 0 { req.depth } else { 1 };

        let subgraph = self.graph_engine
            .neighborhood(node_id, depth)
            .map_err(|e| Status::internal(e.to_string()))?;

        let nodes: Vec<_> = subgraph.nodes.values()
            .map(|n| {
                let edge_count = self.get_edge_count(n.id);
                node_to_response(n, edge_count)
            })
            .collect();

        let edges: Vec<_> = subgraph.edges.iter().map(edge_to_response).collect();

        let depths: std::collections::HashMap<String, u32> = subgraph.depths
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        Ok(Response::new(SubgraphResponse {
            nodes,
            edges,
            depths,
            visited_count: subgraph.visited_count as u32,
            truncated: subgraph.truncated,
        }))
    }

    async fn similarity_search(
        &self,
        request: Request<SimilaritySearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        let req = request.into_inner();

        let embedding = self.embedding_service
            .embed(&req.query)
            .map_err(|e| Status::internal(e.to_string()))?;

        let limit = if req.limit > 0 { req.limit as usize } else { 10 };

        let mut filter = VectorFilter::new();
        if !req.kind_filter.is_empty() {
            let kinds: Result<Vec<_>, _> = req.kind_filter.iter()
                .map(|s| parse_node_kind(s))
                .collect();
            filter = filter.with_kinds(kinds.map_err(|e| Status::invalid_argument(e.to_string()))?);
        }

        let index = self.vector_index.read().unwrap();
        let results = if req.min_score > 0.0 {
            index.search_threshold(&embedding, req.min_score, Some(&filter))
                .map_err(|e| Status::internal(e.to_string()))?
        } else {
            index.search(&embedding, limit, Some(&filter))
                .map_err(|e| Status::internal(e.to_string()))?
        };
        drop(index);

        let search_results: Vec<_> = results.iter()
            .filter_map(|r| {
                self.storage.get_node(r.node_id).ok()
                    .flatten()
                    .map(|node| {
                        let edge_count = self.get_edge_count(node.id);
                        SearchResultEntry {
                            node: Some(node_to_response(&node, edge_count)),
                            score: r.score,
                        }
                    })
            })
            .take(limit)
            .collect();

        Ok(Response::new(SearchResponse {
            results: search_results,
        }))
    }

    async fn hybrid_search(
        &self,
        request: Request<HybridSearchRequest>,
    ) -> Result<Response<HybridSearchResponse>, Status> {
        let req = request.into_inner();

        let anchors: Result<Vec<_>, _> = req.anchor_ids.iter()
            .map(|s| s.parse::<uuid::Uuid>())
            .collect();
        let anchors = anchors.map_err(|e| Status::invalid_argument(format!("Invalid anchor_ids: {}", e)))?;

        let mut query = HybridQuery::new(req.query)
            .with_anchors(anchors)
            .with_vector_weight(if req.vector_weight > 0.0 { req.vector_weight } else { 0.7 })
            .with_limit(if req.limit > 0 { req.limit as usize } else { 10 })
            .with_max_anchor_depth(if req.max_anchor_depth > 0 { req.max_anchor_depth } else { 3 });

        if !req.kind_filter.is_empty() {
            let kinds: Result<Vec<_>, _> = req.kind_filter.iter()
                .map(|s| parse_node_kind(s))
                .collect();
            query = query.with_kind_filter(kinds.map_err(|e| Status::invalid_argument(e.to_string()))?);
        }

        // Arc<E> and Arc<G> implement EmbeddingService/GraphEngine via blanket impls.
        // RwLockVectorIndex wraps Arc<RwLock<V>> to implement VectorIndex.
        let hybrid = HybridSearch::new(
            self.storage.clone(),
            self.embedding_service.clone(),
            RwLockVectorIndex(self.vector_index.clone()),
            self.graph_engine.clone(),
        );

        let results = hybrid.search(query)
            .map_err(|e| Status::internal(e.to_string()))?;

        let hybrid_results: Vec<_> = results.iter()
            .map(|r| {
                let edge_count = self.get_edge_count(r.node.id);
                HybridResultEntry {
                    node: Some(node_to_response(&r.node, edge_count)),
                    vector_score: r.vector_score,
                    graph_score: r.graph_score,
                    combined_score: r.combined_score,
                    nearest_anchor_id: r.nearest_anchor.as_ref().map(|(id, _)| id.to_string()),
                    nearest_anchor_depth: r.nearest_anchor.as_ref().map(|(_, depth)| *depth),
                }
            })
            .collect();

        Ok(Response::new(HybridSearchResponse {
            results: hybrid_results,
        }))
    }

    async fn get_briefing(
        &self,
        _request: Request<BriefingRequest>,
    ) -> Result<Response<BriefingResponse>, Status> {
        // Briefings are Phase 6 - return placeholder for now
        Err(Status::unimplemented("Briefings are coming in Phase 6"))
    }

    async fn stats(
        &self,
        _request: Request<StatsRequest>,
    ) -> Result<Response<StatsResponse>, Status> {
        let stats = self.storage.stats()
            .map_err(|e| Status::internal(e.to_string()))?;

        // Try to get DB file size
        let db_size = std::fs::metadata(self.storage.path())
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(Response::new(stats_to_response(stats, db_size)))
    }

    async fn auto_linker_status(
        &self,
        _request: Request<AutoLinkerStatusRequest>,
    ) -> Result<Response<AutoLinkerStatusResponse>, Status> {
        let linker = self.auto_linker.read().unwrap();
        let metrics = linker.metrics();

        Ok(Response::new(AutoLinkerStatusResponse {
            cycles: metrics.cycles,
            nodes_processed: metrics.nodes_processed,
            edges_created: metrics.edges_created,
            edges_pruned: metrics.edges_pruned,
            edges_deleted: metrics.edges_deleted,
            duplicates_found: metrics.duplicates_found,
            contradictions_found: metrics.contradictions_found,
            last_cycle_duration_ms: metrics.last_cycle_duration.as_millis() as u64,
            cursor: Some(datetime_to_timestamp(metrics.cursor)),
            backlog_size: metrics.backlog_size,
        }))
    }

    async fn trigger_auto_link(
        &self,
        _request: Request<TriggerAutoLinkRequest>,
    ) -> Result<Response<TriggerAutoLinkResponse>, Status> {
        let mut linker = self.auto_linker.write().unwrap();

        match linker.run_cycle() {
            Ok(()) => Ok(Response::new(TriggerAutoLinkResponse {
                success: true,
                message: "Auto-link cycle completed successfully".to_string(),
            })),
            Err(e) => Ok(Response::new(TriggerAutoLinkResponse {
                success: false,
                message: format!("Auto-link cycle failed: {}", e),
            })),
        }
    }

    async fn reindex(
        &self,
        _request: Request<ReindexRequest>,
    ) -> Result<Response<ReindexResponse>, Status> {
        let nodes = self.storage.list_nodes(NodeFilter::new())
            .map_err(|e| Status::internal(e.to_string()))?;

        // Generate all embeddings without holding the write lock — embedding is CPU-bound
        // and can take seconds for large graphs. Holding the lock would block all reads.
        let pairs: Vec<(NodeId, Vec<f32>)> = nodes
            .iter()
            .filter_map(|node| {
                let text = embedding_input(node);
                self.embedding_service.embed(&text).ok()
                    .map(|emb| (node.id, emb))
            })
            .collect();

        let reindexed = pairs.len();

        // Acquire lock only for the fast batch-insert step
        {
            let mut index = self.vector_index.write().unwrap();
            for (id, emb) in &pairs {
                let _ = index.insert(*id, emb);
            }
            if let Err(e) = index.rebuild() {
                return Err(Status::internal(format!("Failed to rebuild index: {}", e)));
            }
        }

        Ok(Response::new(ReindexResponse {
            success: true,
            nodes_reindexed: reindexed as u64,
            message: format!("Reindexed {} nodes", reindexed),
        }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let stats = self.storage.stats()
            .map_err(|e| Status::internal(e.to_string()))?;

        let db_size = std::fs::metadata(self.storage.path())
            .map(|m| m.len())
            .unwrap_or(0);

        let linker = self.auto_linker.read().unwrap();
        let linker_metrics = linker.metrics();

        Ok(Response::new(HealthResponse {
            healthy: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            stats: Some(stats_to_response(stats, db_size)),
            auto_linker: Some(AutoLinkerStatusResponse {
                cycles: linker_metrics.cycles,
                nodes_processed: linker_metrics.nodes_processed,
                edges_created: linker_metrics.edges_created,
                edges_pruned: linker_metrics.edges_pruned,
                edges_deleted: linker_metrics.edges_deleted,
                duplicates_found: linker_metrics.duplicates_found,
                contradictions_found: linker_metrics.contradictions_found,
                last_cycle_duration_ms: linker_metrics.last_cycle_duration.as_millis() as u64,
                cursor: Some(datetime_to_timestamp(linker_metrics.cursor)),
                backlog_size: linker_metrics.backlog_size,
            }),
        }))
    }
}
