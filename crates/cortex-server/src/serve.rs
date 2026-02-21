use crate::config::CortexConfig;
use cortex_core::briefing::{BriefingConfig, BriefingEngine};
use cortex_core::storage::encrypted;
use cortex_core::*;
use cortex_proto::cortex_service_server::CortexServiceServer;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::Duration;
use tokio::task::JoinHandle;
use tonic::transport::Server;
use tracing::{error, info, warn};

/// RAII guard: re-encrypts the temp DB file back to the original encrypted path on drop.
struct EncryptedDbGuard {
    temp_path: std::path::PathBuf,
    encrypted_path: std::path::PathBuf,
    key: [u8; 32],
}

impl Drop for EncryptedDbGuard {
    fn drop(&mut self) {
        if let Err(e) = encrypted::encrypt_file(&self.temp_path, &self.key) {
            eprintln!("ERROR: Failed to re-encrypt database: {}", e);
            return;
        }
        if let Err(e) = std::fs::rename(&self.temp_path, &self.encrypted_path) {
            eprintln!("ERROR: Failed to replace encrypted database file: {}", e);
        }
    }
}

pub async fn run(config: CortexConfig) -> anyhow::Result<()> {
    info!("Starting Cortex server v{}", env!("CARGO_PKG_VERSION"));
    info!("gRPC: {}", config.server.grpc_addr);
    info!("HTTP: {}", config.server.http_addr);
    info!("Data: {:?}", config.server.data_dir);

    // Validate auth config early so we fail fast before opening the database.
    let auth_enabled = config.security.auth_enabled;
    let auth_token = config.security.resolved_token();
    if auth_enabled {
        if auth_token.is_none() {
            return Err(anyhow::anyhow!(
                "[security] auth_enabled = true but no token found. \
                 Set CORTEX_AUTH_TOKEN env var or set auth_token in [security] config."
            ));
        }
        info!("Bearer token auth: enabled");
    } else {
        warn!("Auth disabled — Cortex is open to all connections on {}", config.server.http_addr);
    }

    // Encryption at rest: decrypt to a temp file before opening with redb
    let db_path = config.db_path();
    let (_encrypted_guard, storage_path) = if config.security.encryption {
        let key =
            encrypted::derive_key().map_err(|e| anyhow::anyhow!("Encryption key error: {}", e))?;

        let temp_path = db_path.with_extension("redb.tmp");

        if db_path.exists() {
            info!("Decrypting database to temp file...");
            std::fs::copy(&db_path, &temp_path)?;
            encrypted::decrypt_file(&temp_path, &key)
                .map_err(|e| anyhow::anyhow!("Failed to decrypt database: {}", e))?;
        }

        info!("Encryption at rest enabled — database will be re-encrypted on shutdown.");
        let guard = EncryptedDbGuard {
            temp_path: temp_path.clone(),
            encrypted_path: db_path,
            key,
        };
        (Some(guard), temp_path)
    } else {
        (None, db_path)
    };

    // Initialize storage with audit log
    info!("Opening database...");
    let storage_inner = RedbStorage::open(&storage_path)?;
    let audit_log = Arc::new(storage_inner.create_audit_log());
    let storage = Arc::new(storage_inner.with_audit_log(audit_log));
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

    // Initialize graph version counter
    let graph_version = Arc::new(AtomicU64::new(0));

    // Initialize briefing engine
    info!("Initializing briefing engine...");
    let briefing_engine = Arc::new(BriefingEngine::new(
        storage.clone(),
        graph_engine.clone(),
        RwLockVectorIndex(vector_index.clone()),
        embedding_service.clone(),
        graph_version.clone(),
        BriefingConfig::default(),
    ));
    info!("Briefing engine ready");

    // Start auto-linker background task (also runs retention sweep each cycle)
    let auto_linker_task = {
        let linker = auto_linker.clone();
        let storage_for_retention = storage.clone();
        let interval = auto_linker_config.interval;
        let retention_cfg = config.retention.clone();
        let has_retention = retention_cfg.default_ttl_days > 0
            || !retention_cfg.by_kind.is_empty()
            || retention_cfg.max_nodes.is_some();

        tokio::spawn(async move {
            let retention_engine = if has_retention {
                Some(RetentionEngine::new(retention_cfg))
            } else {
                None
            };

            loop {
                tokio::time::sleep(interval).await;

                {
                    let mut linker = linker.write().unwrap();
                    if let Err(e) = linker.run_cycle() {
                        error!("Auto-linker cycle failed: {}", e);
                    }
                }

                if let Some(ref retention) = retention_engine {
                    match retention.sweep(storage_for_retention.as_ref()) {
                        Ok(0) => {}
                        Ok(n) => info!("Retention: soft-deleted {} nodes", n),
                        Err(e) => error!("Retention sweep failed: {}", e),
                    }
                    match retention.purge_expired(storage_for_retention.as_ref()) {
                        Ok(0) => {}
                        Ok(n) => info!("Retention: hard-deleted {} expired nodes", n),
                        Err(e) => error!("Retention purge failed: {}", e),
                    }
                }
            }
        })
    };

    // Start briefing precomputer
    let precompute_agents = if config.briefing.precompute_agents.is_empty() {
        std::env::var("CORTEX_BRIEFING_AGENTS")
            .unwrap_or_else(|_| "kai,dutybound".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    } else {
        config.briefing.precompute_agents.clone()
    };

    let _precomputer_task = {
        let engine = briefing_engine.clone();
        let agents = precompute_agents.clone();
        tokio::spawn(async move {
            crate::briefing::BriefingPrecomputer::new(engine, agents, Duration::from_secs(60))
                .run()
                .await;
        })
    };

    // Optionally start file ingest loop
    let ingest_dir = config
        .ingest
        .file
        .as_ref()
        .map(|f| f.watch_dir.clone())
        .or_else(|| std::env::var("CORTEX_INGEST_DIR").ok().map(Into::into));

    let _ingest_task: Option<JoinHandle<()>> = if let Some(ingest_path) = ingest_dir {
        info!("File ingest enabled, watching {:?}", ingest_path);

        let ingestor = cortex_core::briefing::ingest::FileIngest::new(
            ingest_path,
            storage.clone(),
            embedding_service.clone(),
            vector_index.clone(),
            graph_version.clone(),
        );

        Some(tokio::spawn(async move {
            loop {
                match ingestor.scan_once() {
                    Ok(n) if n > 0 => info!("File ingest: created {} nodes", n),
                    Err(e) => error!("File ingest error: {}", e),
                    _ => {}
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }))
    } else {
        None
    };

    // Start gRPC server
    let grpc_task = {
        let grpc_service = crate::grpc::CortexServiceImpl::new(
            storage.clone(),
            graph_engine.clone(),
            vector_index.clone(),
            embedding_service.clone(),
            auto_linker.clone(),
            graph_version.clone(),
            briefing_engine.clone(),
        );

        let addr = config.grpc_addr();
        let grpc_auth_enabled = auth_enabled;
        let grpc_auth_token = auth_token.clone().map(|t| format!("Bearer {}", t));

        tokio::spawn(async move {
            info!("Starting gRPC server on {}", addr);
            let svc = CortexServiceServer::with_interceptor(
                grpc_service,
                move |req: tonic::Request<()>| {
                    if !grpc_auth_enabled {
                        return Ok(req);
                    }
                    let expected = grpc_auth_token.as_deref().unwrap_or("");
                    match req.metadata().get("authorization").and_then(|v| v.to_str().ok()) {
                        Some(t) if t == expected => Ok(req),
                        _ => Err(tonic::Status::unauthenticated("Invalid or missing token")),
                    }
                },
            );
            Server::builder()
                .add_service(svc)
                .serve(addr)
                .await
                .expect("gRPC server failed");
        })
    };

    // Start HTTP server
    let http_task = {
        let app_state = crate::http::AppState {
            storage: storage.clone(),
            graph_engine: graph_engine.clone(),
            vector_index: vector_index.clone(),
            embedding_service: embedding_service.clone(),
            auto_linker: auto_linker.clone(),
            graph_version: graph_version.clone(),
            briefing_engine: briefing_engine.clone(),
            start_time: std::time::Instant::now(),
            rollback_config: config.prompt_rollback.clone(),
            webhooks: config.webhooks.clone(),
            score_decay: config.score_decay.clone(),
            write_gate: config.write_gate.clone(),
        };

        let http_auth_token = auth_token.clone();
        let app = crate::http::create_router(app_state).layer(axum::middleware::from_fn(
            move |req, next| {
                let tok = http_auth_token.clone();
                async move { crate::http::auth::check(req, next, auth_enabled, tok).await }
            },
        ));
        let addr = config.http_addr();

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
    let nats_enabled = config.server.nats_enabled;
    let nats_url = config.server.nats_url.clone();

    let nats_task: Option<JoinHandle<()>> = if nats_enabled {
        info!("Connecting to NATS at {}...", nats_url);

        #[cfg(feature = "warren")]
        {
            match async_nats::connect(&nats_url).await {
                Ok(client) => {
                    info!("NATS connected (Warren adapter)");
                    let nats_ingest = crate::nats::NatsIngest::new(
                        client,
                        storage.clone(),
                        embedding_service.clone(),
                        vector_index.clone(),
                        graph_version.clone(),
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
        }

        #[cfg(not(feature = "warren"))]
        {
            info!("NATS consumer not available (warren feature disabled)");
            None
        }
    } else {
        info!("NATS consumer disabled");
        None
    };

    info!("Cortex server ready");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, terminating...");

    grpc_task.abort();
    http_task.abort();
    auto_linker_task.abort();
    if let Some(task) = nats_task {
        task.abort();
    }

    Ok(())
}
