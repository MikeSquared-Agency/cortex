use crate::error::{CortexError, Result};
use crate::storage::Storage;
use crate::types::{Node, NodeKind, Source};
use crate::vector::EmbeddingService;
use crate::vector::VectorIndex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Scans a directory for `.md`/`.txt` files, chunks them into nodes,
/// generates embeddings, and stores them. Processed files are moved to
/// `{watch_dir}/processed/`.
pub struct FileIngest<S: Storage, E: EmbeddingService, V: VectorIndex> {
    pub watch_dir: PathBuf,
    storage: Arc<S>,
    embeddings: E,
    vector_index: Arc<RwLock<V>>,
    graph_version: Arc<AtomicU64>,
}

impl<S: Storage, E: EmbeddingService, V: VectorIndex> FileIngest<S, E, V> {
    pub fn new(
        watch_dir: PathBuf,
        storage: Arc<S>,
        embeddings: E,
        vector_index: Arc<RwLock<V>>,
        graph_version: Arc<AtomicU64>,
    ) -> Self {
        Self {
            watch_dir,
            storage,
            embeddings,
            vector_index,
            graph_version,
        }
    }

    /// Scan once. Returns the number of nodes created.
    pub fn scan_once(&self) -> Result<usize> {
        let mut created = 0;

        let entries = std::fs::read_dir(&self.watch_dir)
            .map_err(|e| CortexError::Validation(format!("read_dir failed: {}", e)))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "md" | "txt") {
                continue;
            }

            match self.process_file(&path) {
                Ok(n) => {
                    created += n;
                    // Move to processed/
                    let processed_dir = self.watch_dir.join("processed");
                    let _ = std::fs::create_dir_all(&processed_dir);
                    if let Some(fname) = path.file_name() {
                        let dest = processed_dir.join(fname);
                        let _ = std::fs::rename(&path, &dest);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to process {:?}: {}", path, e);
                }
            }
        }

        Ok(created)
    }

    fn process_file(&self, path: &Path) -> Result<usize> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CortexError::Validation(format!("read_to_string failed: {}", e)))?;

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let chunks = if ext == "md" {
            Self::chunk_markdown(&text)
        } else {
            Self::chunk_plain(&text)
        };

        let source_agent = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ingest")
            .to_string();

        let mut created = 0;

        for chunk in &chunks {
            if chunk.trim().is_empty() {
                continue;
            }

            let kind = classify_chunk(chunk);
            let raw_title = chunk.lines().next().unwrap_or("Untitled").trim().to_string();
            let title = raw_title.trim_start_matches('#').trim().to_string();
            let title = if title.len() > 200 {
                title[..200].to_string()
            } else {
                title
            };

            let source = Source {
                agent: source_agent.clone(),
                session: None,
                channel: Some("ingest".to_string()),
            };

            let mut node = Node::new(kind, title, chunk.clone(), source, 0.5);

            match self.embeddings.embed(chunk) {
                Ok(embedding) => {
                    node.embedding = Some(embedding.clone());
                    self.storage.put_node(&node)?;
                    {
                        let mut index = self.vector_index.write().unwrap();
                        let _ = index.insert(node.id, &embedding);
                    }
                }
                Err(_) => {
                    self.storage.put_node(&node)?;
                }
            }

            created += 1;
        }

        // Bump the version once per file, not once per chunk, to avoid
        // invalidating the briefing cache on every individual chunk write.
        if created > 0 {
            self.graph_version.fetch_add(1, Ordering::Relaxed);
        }

        Ok(created)
    }

    /// Split markdown into sections by heading (`#`).
    fn chunk_markdown(text: &str) -> Vec<String> {
        let mut chunks: Vec<String> = Vec::new();
        let mut current = String::new();

        for line in text.lines() {
            if line.starts_with('#') && !current.is_empty() {
                chunks.push(current.trim().to_string());
                current = String::new();
            }
            current.push_str(line);
            current.push('\n');
        }

        if !current.trim().is_empty() {
            chunks.push(current.trim().to_string());
        }

        chunks
    }

    /// Split plain text into 20-line groups.
    fn chunk_plain(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        lines
            .chunks(20)
            .map(|chunk| chunk.join("\n"))
            .filter(|s| !s.trim().is_empty())
            .collect()
    }
}

/// Heuristic classifier — maps chunk text to the most likely NodeKind.
// Public for testing and external use.
pub fn classify_chunk(text: &str) -> NodeKind {
    let lower = text.to_lowercase();
    if lower.contains("decided")
        || lower.contains("decision")
        || lower.contains("chose")
        || lower.contains("will use")
    {
        NodeKind::Decision
    } else if lower.contains("goal")
        || lower.contains("target")
        || lower.contains("aim")
        || lower.contains("objective")
    {
        NodeKind::Goal
    } else if lower.contains("prefer")
        || lower.contains("always")
        || lower.contains("never")
        || lower.contains("style")
    {
        NodeKind::Preference
    } else if lower.contains("pattern")
        || lower.contains("recurring")
        || lower.contains("tendency")
    {
        NodeKind::Pattern
    } else if lower.contains("happened")
        || lower.contains("event")
        || lower.contains("occurred")
    {
        NodeKind::Event
    } else if lower.contains("observed")
        || lower.contains("noticed")
        || lower.contains("note")
    {
        NodeKind::Observation
    } else {
        NodeKind::Fact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RedbStorage;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    // --- Minimal no-op implementations for testing without a real model ---

    #[derive(Clone)]
    struct NoopEmbedder;

    impl EmbeddingService for NoopEmbedder {
        fn embed(&self, _text: &str) -> crate::error::Result<crate::types::Embedding> {
            Ok(vec![0.0; 4])
        }
        fn embed_batch(
            &self,
            texts: &[String],
        ) -> crate::error::Result<Vec<crate::types::Embedding>> {
            Ok(texts.iter().map(|_| vec![0.0; 4]).collect())
        }
        fn dimension(&self) -> usize {
            4
        }
        fn model_name(&self) -> &str {
            "noop"
        }
    }

    struct NoopIndex;

    impl VectorIndex for NoopIndex {
        fn insert(
            &mut self,
            _id: crate::types::NodeId,
            _embedding: &crate::types::Embedding,
        ) -> crate::error::Result<()> {
            Ok(())
        }
        fn remove(&mut self, _id: crate::types::NodeId) -> crate::error::Result<()> {
            Ok(())
        }
        fn search(
            &self,
            _query: &crate::types::Embedding,
            _k: usize,
            _filter: Option<&crate::vector::VectorFilter>,
        ) -> crate::error::Result<Vec<crate::vector::SimilarityResult>> {
            Ok(vec![])
        }
        fn search_threshold(
            &self,
            _query: &crate::types::Embedding,
            _threshold: f32,
            _filter: Option<&crate::vector::VectorFilter>,
        ) -> crate::error::Result<Vec<crate::vector::SimilarityResult>> {
            Ok(vec![])
        }
        fn search_batch(
            &self,
            queries: &[(crate::types::NodeId, crate::types::Embedding)],
            _k: usize,
            _filter: Option<&crate::vector::VectorFilter>,
        ) -> crate::error::Result<std::collections::HashMap<crate::types::NodeId, Vec<crate::vector::SimilarityResult>>> {
            Ok(queries.iter().map(|(id, _)| (*id, vec![])).collect())
        }
        fn len(&self) -> usize {
            0
        }
        fn rebuild(&mut self) -> crate::error::Result<()> {
            Ok(())
        }
        fn save(&self, _path: &Path) -> crate::error::Result<()> {
            Ok(())
        }
        fn load(_path: &Path) -> crate::error::Result<Self> {
            Ok(NoopIndex)
        }
    }

    fn make_ingest(dir: &TempDir) -> FileIngest<RedbStorage, NoopEmbedder, NoopIndex> {
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());
        let vector_index = Arc::new(RwLock::new(NoopIndex));
        let graph_version = Arc::new(AtomicU64::new(0));
        FileIngest::new(
            dir.path().to_path_buf(),
            storage,
            NoopEmbedder,
            vector_index,
            graph_version,
        )
    }

    // --- classify_chunk tests ---

    #[test]
    fn test_classify_chunk_decision() {
        assert_eq!(classify_chunk("We decided to use Rust"), NodeKind::Decision);
        assert_eq!(classify_chunk("The decision was final"), NodeKind::Decision);
        assert_eq!(classify_chunk("We chose Tokio"), NodeKind::Decision);
        assert_eq!(classify_chunk("We will use async/await"), NodeKind::Decision);
    }

    #[test]
    fn test_classify_chunk_goal() {
        assert_eq!(classify_chunk("Our goal is to ship v1"), NodeKind::Goal);
        assert_eq!(classify_chunk("Target: 100ms latency"), NodeKind::Goal);
        assert_eq!(classify_chunk("The aim of this project"), NodeKind::Goal);
        assert_eq!(classify_chunk("Objective: reduce cost"), NodeKind::Goal);
    }

    #[test]
    fn test_classify_chunk_preference() {
        assert_eq!(classify_chunk("I prefer async code"), NodeKind::Preference);
        assert_eq!(classify_chunk("Always use rustfmt"), NodeKind::Preference);
        assert_eq!(classify_chunk("Never commit secrets"), NodeKind::Preference);
        assert_eq!(classify_chunk("Coding style: idiomatic"), NodeKind::Preference);
    }

    #[test]
    fn test_classify_chunk_pattern() {
        assert_eq!(classify_chunk("A recurring issue in PRs"), NodeKind::Pattern);
        assert_eq!(classify_chunk("There is a pattern here"), NodeKind::Pattern);
        assert_eq!(classify_chunk("A tendency to over-engineer"), NodeKind::Pattern);
    }

    #[test]
    fn test_classify_chunk_event() {
        assert_eq!(classify_chunk("It happened last Tuesday"), NodeKind::Event);
        assert_eq!(classify_chunk("An event occurred at 3pm"), NodeKind::Event);
    }

    #[test]
    fn test_classify_chunk_observation() {
        assert_eq!(classify_chunk("I observed slow queries"), NodeKind::Observation);
        assert_eq!(classify_chunk("Noticed higher latency"), NodeKind::Observation);
        assert_eq!(classify_chunk("Note: cache hit rate dropped"), NodeKind::Observation);
    }

    #[test]
    fn test_classify_chunk_default_fact() {
        // No keywords → Fact
        assert_eq!(classify_chunk("Rust 1.78 was released"), NodeKind::Fact);
        assert_eq!(classify_chunk("The server listens on port 8080"), NodeKind::Fact);
        assert_eq!(classify_chunk(""), NodeKind::Fact);
    }

    // --- chunk_markdown tests ---

    #[test]
    fn test_chunk_markdown_splits_on_headings() {
        let text = "# Section A\ncontent a\n## Section B\ncontent b\n# Section C\ncontent c";
        let chunks = FileIngest::<RedbStorage, NoopEmbedder, NoopIndex>::chunk_markdown(text);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].contains("Section A"));
        assert!(chunks[1].contains("Section B"));
        assert!(chunks[2].contains("Section C"));
    }

    #[test]
    fn test_chunk_markdown_single_section() {
        let text = "# Only one\nsome content here";
        let chunks = FileIngest::<RedbStorage, NoopEmbedder, NoopIndex>::chunk_markdown(text);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_markdown_no_headings() {
        let text = "no headings at all\njust plain text";
        let chunks = FileIngest::<RedbStorage, NoopEmbedder, NoopIndex>::chunk_markdown(text);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_plain_splits_into_20_line_groups() {
        let text = (0..50).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = FileIngest::<RedbStorage, NoopEmbedder, NoopIndex>::chunk_plain(&text);
        assert_eq!(chunks.len(), 3); // 50 lines / 20 = 3 chunks (last partial)
        // Each chunk except the last should have 20 lines
        assert_eq!(chunks[0].lines().count(), 20);
        assert_eq!(chunks[1].lines().count(), 20);
        assert_eq!(chunks[2].lines().count(), 10);
    }

    // --- FileIngest integration tests (no model) ---

    #[test]
    fn test_file_ingest_creates_nodes_from_markdown() {
        let dir = TempDir::new().unwrap();
        let ingest = make_ingest(&dir);

        // Write a markdown file with two sections
        std::fs::write(
            dir.path().join("test.md"),
            "# Memory\nWe decided to use Rust.\n# Goals\nShip by Q3.",
        )
        .unwrap();

        let created = ingest.scan_once().unwrap();
        assert_eq!(created, 2, "Expected 2 nodes (one per section)");
    }

    #[test]
    fn test_file_ingest_creates_nodes_from_txt() {
        let dir = TempDir::new().unwrap();
        let ingest = make_ingest(&dir);

        // 25 lines → 2 chunks (20 + 5)
        let content = (0..25).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.path().join("notes.txt"), content).unwrap();

        let created = ingest.scan_once().unwrap();
        assert_eq!(created, 2);
    }

    #[test]
    fn test_file_ingest_moves_processed_files() {
        let dir = TempDir::new().unwrap();
        let ingest = make_ingest(&dir);

        let src = dir.path().join("memo.md");
        std::fs::write(&src, "# Note\nSome fact here.").unwrap();

        ingest.scan_once().unwrap();

        assert!(!src.exists(), "Source file should have been moved");
        assert!(
            dir.path().join("processed").join("memo.md").exists(),
            "File should be in processed/"
        );
    }

    #[test]
    fn test_file_ingest_skips_non_text_files() {
        let dir = TempDir::new().unwrap();
        let ingest = make_ingest(&dir);

        std::fs::write(dir.path().join("image.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(dir.path().join("data.json"), r#"{"key":"value"}"#).unwrap();

        let created = ingest.scan_once().unwrap();
        assert_eq!(created, 0, "Non-.md/.txt files must be skipped");
    }

    #[test]
    fn test_file_ingest_graph_version_bumped_once_per_file() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());
        let vector_index = Arc::new(RwLock::new(NoopIndex));
        let graph_version = Arc::new(AtomicU64::new(0));

        let ingest = FileIngest::new(
            dir.path().to_path_buf(),
            storage,
            NoopEmbedder,
            vector_index,
            graph_version.clone(),
        );

        // Write a file with 3 sections → 3 chunks, but version should only bump once
        std::fs::write(
            dir.path().join("multi.md"),
            "# A\ncontent\n# B\ncontent\n# C\ncontent",
        )
        .unwrap();

        let created = ingest.scan_once().unwrap();
        assert_eq!(created, 3);
        assert_eq!(
            graph_version.load(Ordering::Relaxed),
            1,
            "graph_version should increment once per file, not once per chunk"
        );
    }

    #[test]
    fn test_file_ingest_empty_file_no_nodes() {
        let dir = TempDir::new().unwrap();
        let ingest = make_ingest(&dir);

        std::fs::write(dir.path().join("empty.md"), "").unwrap();
        let created = ingest.scan_once().unwrap();
        assert_eq!(created, 0);
    }

    #[test]
    fn test_file_ingest_source_agent_is_filename_stem() {
        let dir = TempDir::new().unwrap();
        let storage = Arc::new(RedbStorage::open(dir.path().join("t.redb")).unwrap());
        let vector_index = Arc::new(RwLock::new(NoopIndex));
        let graph_version = Arc::new(AtomicU64::new(0));

        let ingest = FileIngest::new(
            dir.path().to_path_buf(),
            storage.clone(),
            NoopEmbedder,
            vector_index,
            graph_version,
        );

        std::fs::write(dir.path().join("kai.md"), "# Fact\nKai prefers async.").unwrap();
        ingest.scan_once().unwrap();

        let nodes = storage
            .list_nodes(crate::storage::NodeFilter::new())
            .unwrap();
        assert!(
            nodes.iter().all(|n| n.source.agent == "kai"),
            "source_agent should be the file stem 'kai'"
        );
    }
}
