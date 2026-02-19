#![allow(dead_code)]
mod config;
mod grpc;
mod http;
mod nats;
mod migration;

use clap::Parser;
use config::Config;
use cortex_core::*;
// cortex_core::* brings in a 1-arg Result alias; re-import std's 2-arg form
// so that `Result<(), Box<dyn Error>>` and `?` conversions work correctly.
use std::result::Result;
use cortex_proto::cortex_service_server::CortexServiceServer;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::task::JoinHandle;
use tonic::transport::Server;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Parse config
    let config = Config::parse();
    config.validate()?;

    info!("Starting Cortex server v{}", env!("CARGO_PKG_VERSION"));
    info!("gRPC: {}", config.grpc_addr);
    info!("HTTP: {}", config.http_addr);
    info!("Data: {:?}", config.data_dir);

    // Initialize storage
    info!("Opening database...");
    let storage = Arc::new(RedbStorage::open(config.db_path())?);
    let stats = storage.stats()?;
    info!(
        "Database loaded: {} nodes, {} edges",
        stats.node_count, stats.edge_count
    );

    // Initialize embedding service
    info!("Loading embedding model...");
    let embedding_service = Arc::new(FastEmbedService::new()?);
    info!("Embedding model loaded: {}", embedding_service.model_name());

    // Initialize vector index
    info!("Initializing vector index...");
    let vector_index = Arc::new(StdRwLock::new(HnswIndex::new(
        embedding_service.dimension(),
    )));

    // Rebuild index from existing nodes
    {
        let nodes = storage.list_nodes(NodeFilter::new())?;
        let mut index = vector_index.write().unwrap();
        let mut indexed = 0;

        for node in nodes {
            if let Some(emb) = &node.embedding {
                if index.insert(node.id, emb).is_ok() {
                    indexed += 1;
                }
            }
        }

        if indexed > 0 {
            index.rebuild()?;
            info!("Indexed {} node embeddings", indexed);
        }
    }

    // Initialize graph engine
    let graph_engine = Arc::new(GraphEngineImpl::new(storage.clone()));

    // Initialize auto-linker
    info!("Initializing auto-linker...");
    let auto_linker_config = config.auto_linker_config();
    let auto_linker = Arc::new(StdRwLock::new(AutoLinker::new(
        storage.clone(),
        graph_engine.clone(),
        vector_index.clone(),
        embedding_service.clone(),
        auto_linker_config.clone(),
    )?));

    info!(
        "Auto-linker initialized (interval: {}s)",
        auto_linker_config.interval.as_secs()
    );

    // Start auto-linker background task
    let auto_linker_task = {
        let linker = auto_linker.clone();
        let interval = auto_linker_config.interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;

                let mut linker = linker.write().unwrap();
                if let Err(e) = linker.run_cycle() {
                    error!("Auto-linker cycle failed: {}", e);
                }
            }
        })
    };

    // Start gRPC server
    let grpc_task = {
        let grpc_service = grpc::CortexServiceImpl::new(
            storage.clone(),
            graph_engine.clone(),
            vector_index.clone(),
            embedding_service.clone(),
            auto_linker.clone(),
        );

        let addr = config.grpc_addr;

        tokio::spawn(async move {
            info!("Starting gRPC server on {}", addr);

            Server::builder()
                .add_service(CortexServiceServer::new(grpc_service))
                .serve(addr)
                .await
                .expect("gRPC server failed");
        })
    };

    // Start HTTP server
    let http_task = {
        let app_state = http::AppState {
            storage: storage.clone(),
            graph_engine: graph_engine.clone(),
            vector_index: vector_index.clone(),
            embedding_service: embedding_service.clone(),
            auto_linker: auto_linker.clone(),
            start_time: std::time::Instant::now(),
        };

        let app = http::create_router(app_state);
        let addr = config.http_addr;

        tokio::spawn(async move {
            info!("Starting HTTP server on {}", addr);

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("Failed to bind HTTP server");

            axum::serve(listener, app)
                .await
                .expect("HTTP server failed");
        })
    };

    // Optionally start NATS consumer
    let nats_task: Option<JoinHandle<()>> = if config.nats_enabled {
        info!("Connecting to NATS at {}...", config.nats_url);

        match async_nats::connect(&config.nats_url).await {
            Ok(client) => {
                info!("NATS connected");

                let nats_ingest = nats::NatsIngest::new(
                    client,
                    storage.clone(),
                    embedding_service.clone(),
                    vector_index.clone(),
                );

                Some(tokio::spawn(async move {
                    if let Err(e) = nats_ingest.start().await {
                        error!("NATS ingest failed: {}", e);
                    }
                }))
            }
            Err(e) => {
                error!("Failed to connect to NATS: {}", e);
                error!("Continuing without NATS consumer");
                None
            }
        }
    } else {
        info!("NATS consumer disabled");
        None
    };

    info!("Cortex server ready");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, terminating...");

    // Cancel tasks
    grpc_task.abort();
    http_task.abort();
    auto_linker_task.abort();
    if let Some(task) = nats_task {
        task.abort();
    }

    Ok(())
}
