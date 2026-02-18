// Integration tests are in the individual modules (embedding, index, hybrid, config)
// This file is for cross-module integration tests

#[cfg(test)]
mod integration_tests {
    
    use crate::storage::{RedbStorage, Storage};
    use crate::types::*;
    use crate::vector::*;
    
    use tempfile::TempDir;

    fn create_test_node(kind: NodeKind, title: &str, body: &str) -> Node {
        Node::new(
            kind,
            title.to_string(),
            body.to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    #[test]
    #[ignore] // Requires model download - run with: cargo test -- --ignored
    fn test_full_vector_pipeline() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("vector_pipeline.redb");

        let storage = RedbStorage::open(&db_path).unwrap();

        // Create test nodes
        let rust_node = create_test_node(
            NodeKind::Fact,
            "Rust programming",
            "Rust is a systems programming language focused on safety and performance",
        );

        let python_node = create_test_node(
            NodeKind::Fact,
            "Python programming",
            "Python is a high-level interpreted programming language",
        );

        let cooking_node = create_test_node(
            NodeKind::Fact,
            "Cooking pasta",
            "Pasta should be cooked in boiling salted water",
        );

        storage.put_node(&rust_node).unwrap();
        storage.put_node(&python_node).unwrap();
        storage.put_node(&cooking_node).unwrap();

        // Create embeddings
        let embedding_service = FastEmbedService::new().unwrap();
        let mut vector_index = HnswIndex::new(384);

        for node in [&rust_node, &python_node, &cooking_node] {
            let input_text = embedding_input(node);
            let embedding = embedding_service.embed(&input_text).unwrap();
            vector_index
                .set_metadata(node.id, node.kind, node.source.agent.clone());
            vector_index.insert(node.id, &embedding).unwrap();
        }

        vector_index.rebuild().unwrap();

        // Test similarity search
        let query_embedding = embedding_service
            .embed("programming languages")
            .unwrap();

        let results = vector_index
            .search(&query_embedding, 2, None)
            .unwrap();

        assert_eq!(results.len(), 2);

        // Programming nodes should rank higher than cooking
        let result_ids: Vec<_> = results.iter().map(|r| r.node_id).collect();
        assert!(result_ids.contains(&rust_node.id) || result_ids.contains(&python_node.id));
        assert!(!result_ids.contains(&cooking_node.id));
    }

    #[test]
    #[ignore] // Requires model download
    fn test_vector_index_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("persist_test.redb");
        let index_path = temp_dir.path().join("test.hnsw");

        let storage = RedbStorage::open(&db_path).unwrap();

        let node = create_test_node(
            NodeKind::Fact,
            "Test node",
            "This is a test node for persistence",
        );
        storage.put_node(&node).unwrap();

        let embedding_service = FastEmbedService::new().unwrap();
        let mut vector_index = HnswIndex::new(384);

        let embedding = embedding_service
            .embed(&embedding_input(&node))
            .unwrap();

        vector_index.insert(node.id, &embedding).unwrap();
        vector_index.rebuild().unwrap();

        // Save index
        vector_index.save(&index_path).unwrap();

        // Load index
        let loaded_index = HnswIndex::load(&index_path).unwrap();

        assert_eq!(loaded_index.len(), 1);

        // Search should work
        let results = loaded_index.search(&embedding, 1, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, node.id);
    }

    #[test]
    fn test_similarity_config_validation() {
        let valid_config = SimilarityConfig::new()
            .with_auto_link_threshold(0.75)
            .with_dedup_threshold(0.92);

        assert!(valid_config.validate().is_ok());

        let invalid_config = SimilarityConfig::new()
            .with_auto_link_threshold(0.95)
            .with_dedup_threshold(0.90);

        assert!(invalid_config.validate().is_err());
    }
}
