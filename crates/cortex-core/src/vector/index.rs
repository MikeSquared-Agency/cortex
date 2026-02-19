use crate::error::{CortexError, Result};
use crate::types::{Embedding, NodeId, NodeKind};
use instant_distance::{Builder, HnswMap, Point, Search};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Result from a similarity search
#[derive(Debug, Clone)]
pub struct SimilarityResult {
    pub node_id: NodeId,
    pub score: f32,    // Cosine similarity, 0.0 to 1.0
    pub distance: f32, // 1.0 - score
}

/// Filter for vector searches
#[derive(Debug, Clone, Default)]
pub struct VectorFilter {
    /// Only search within these node kinds.
    pub kinds: Option<Vec<NodeKind>>,
    /// Exclude these specific node IDs.
    pub exclude: Option<Vec<NodeId>>,
    /// Only include nodes from this agent.
    pub source_agent: Option<String>,
}

impl VectorFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_kinds(mut self, kinds: Vec<NodeKind>) -> Self {
        self.kinds = Some(kinds);
        self
    }

    pub fn excluding(mut self, ids: Vec<NodeId>) -> Self {
        self.exclude = Some(ids);
        self
    }

    pub fn with_source_agent(mut self, agent: String) -> Self {
        self.source_agent = Some(agent);
        self
    }
}

/// Trait for vector similarity search
pub trait VectorIndex: Send + Sync {
    /// Add a vector with associated node ID.
    fn insert(&mut self, id: NodeId, embedding: &Embedding) -> Result<()>;

    /// Remove a vector.
    fn remove(&mut self, id: NodeId) -> Result<()>;

    /// Find the K nearest neighbors to a query vector.
    fn search(
        &self,
        query: &Embedding,
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>>;

    /// Find all vectors within a similarity threshold.
    fn search_threshold(
        &self,
        query: &Embedding,
        threshold: f32,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>>;

    /// Batch search for auto-linker efficiency.
    fn search_batch(
        &self,
        queries: &[(NodeId, Embedding)],
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<HashMap<NodeId, Vec<SimilarityResult>>>;

    /// Number of vectors in the index.
    fn len(&self) -> usize;

    /// Check if index is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rebuild the index from scratch (after bulk inserts).
    fn rebuild(&mut self) -> Result<()>;

    /// Save index to disk.
    fn save(&self, path: &Path) -> Result<()>;

    /// Load index from disk.
    fn load(path: &Path) -> Result<Self>
    where
        Self: Sized;
}

/// Wrapper that implements VectorIndex for Arc<RwLock<V>>.
/// Allows using a shared, mutex-guarded index (e.g. from a gRPC service)
/// with HybridSearch without requiring Clone on the underlying index.
pub struct RwLockVectorIndex<V: VectorIndex>(pub std::sync::Arc<std::sync::RwLock<V>>);

impl<V: VectorIndex> Clone for RwLockVectorIndex<V> {
    fn clone(&self) -> Self {
        RwLockVectorIndex(std::sync::Arc::clone(&self.0))
    }
}

impl<V: VectorIndex> VectorIndex for RwLockVectorIndex<V> {
    fn insert(&mut self, id: NodeId, embedding: &Embedding) -> Result<()> {
        self.0.write().unwrap().insert(id, embedding)
    }
    fn remove(&mut self, id: NodeId) -> Result<()> {
        self.0.write().unwrap().remove(id)
    }
    fn search(
        &self,
        query: &Embedding,
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>> {
        self.0.read().unwrap().search(query, k, filter)
    }
    fn search_threshold(
        &self,
        query: &Embedding,
        threshold: f32,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>> {
        self.0
            .read()
            .unwrap()
            .search_threshold(query, threshold, filter)
    }
    fn search_batch(
        &self,
        queries: &[(NodeId, Embedding)],
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<HashMap<NodeId, Vec<SimilarityResult>>> {
        self.0.read().unwrap().search_batch(queries, k, filter)
    }
    fn len(&self) -> usize {
        self.0.read().unwrap().len()
    }
    fn rebuild(&mut self) -> Result<()> {
        self.0.write().unwrap().rebuild()
    }
    fn save(&self, path: &Path) -> Result<()> {
        self.0.read().unwrap().save(path)
    }
    fn load(_path: &Path) -> Result<Self>
    where
        Self: Sized,
    {
        Err(CortexError::Validation(
            "load() is not supported for RwLockVectorIndex".to_string(),
        ))
    }
}

/// Wrapper for embeddings to implement Point trait
#[derive(Clone, Debug)]
struct EmbeddingPoint(Vec<f32>);

impl Point for EmbeddingPoint {
    fn distance(&self, other: &Self) -> f32 {
        // Cosine distance = 1 - cosine similarity
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        let norm_a: f32 = self.0.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.0.iter().map(|x| x * x).sum::<f32>().sqrt();

        let similarity = dot / (norm_a * norm_b);
        1.0 - similarity
    }
}

/// HNSW-based vector index implementation
pub struct HnswIndex {
    /// The HNSW index
    index: Option<HnswMap<EmbeddingPoint, NodeId>>,

    /// Raw data for rebuilding
    vectors: HashMap<NodeId, Vec<f32>>,

    /// Metadata for filtering (node kind, source agent)
    metadata: HashMap<NodeId, NodeMetadata>,

    /// Embedding dimension
    dimension: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct NodeMetadata {
    kind: NodeKind,
    source_agent: String,
}

impl HnswIndex {
    /// Create a new empty HNSW index
    pub fn new(dimension: usize) -> Self {
        Self {
            index: None,
            vectors: HashMap::new(),
            metadata: HashMap::new(),
            dimension,
        }
    }

    /// Create index with metadata for filtering
    pub fn with_metadata(dimension: usize) -> Self {
        Self::new(dimension)
    }

    /// Set metadata for a node
    pub fn set_metadata(&mut self, id: NodeId, kind: NodeKind, source_agent: String) {
        self.metadata
            .insert(id, NodeMetadata { kind, source_agent });
    }

    /// Check if a result matches the filter
    fn matches_filter(&self, id: &NodeId, filter: &VectorFilter) -> bool {
        // Check exclusion list
        if let Some(ref exclude) = filter.exclude {
            if exclude.contains(id) {
                return false;
            }
        }

        // If we have metadata for this node, check filters
        if let Some(meta) = self.metadata.get(id) {
            // Check kind filter
            if let Some(ref kinds) = filter.kinds {
                if !kinds.contains(&meta.kind) {
                    return false;
                }
            }

            // Check source agent filter
            if let Some(ref agent) = filter.source_agent {
                if meta.source_agent != *agent {
                    return false;
                }
            }
        }

        true
    }

    /// Convert distance to similarity score
    fn distance_to_similarity(distance: f32) -> f32 {
        (1.0 - distance).max(0.0).min(1.0)
    }

    /// Brute-force fallback search when HNSW index hasn't been built yet
    fn brute_force_search(
        &self,
        query: &Embedding,
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>> {
        let query_point = EmbeddingPoint(query.clone());
        let mut results: Vec<SimilarityResult> = self
            .vectors
            .iter()
            .map(|(id, vec)| {
                let distance = query_point.distance(&EmbeddingPoint(vec.clone()));
                (*id, distance)
            })
            .filter(|(id, _)| {
                if let Some(f) = filter {
                    self.matches_filter(id, f)
                } else {
                    true
                }
            })
            .map(|(id, distance)| SimilarityResult {
                node_id: id,
                score: Self::distance_to_similarity(distance),
                distance,
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(k);
        Ok(results)
    }
}

impl VectorIndex for HnswIndex {
    fn insert(&mut self, id: NodeId, embedding: &Embedding) -> Result<()> {
        if embedding.len() != self.dimension {
            return Err(CortexError::Validation(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            )));
        }

        self.vectors.insert(id, embedding.clone());

        // Index becomes stale after inserts, but we keep it usable.
        // It will still return results for previously-indexed vectors.
        // Call rebuild() to include newly inserted vectors in search results.

        Ok(())
    }

    fn remove(&mut self, id: NodeId) -> Result<()> {
        self.vectors.remove(&id);
        self.metadata.remove(&id);
        // Don't nuke the index on every removal — batch removals
        // and call rebuild() when done. The stale index may return
        // results for removed nodes; callers should check node existence.
        Ok(())
    }

    fn search(
        &self,
        query: &Embedding,
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>> {
        if self.vectors.is_empty() {
            return Ok(Vec::new());
        }

        // Auto-rebuild if index doesn't exist yet
        // Note: this is a read-path rebuild. For mutable self, caller should
        // use rebuild() explicitly. We use a fallback brute-force search.
        if self.index.is_none() {
            return self.brute_force_search(query, k, filter);
        }

        let index = self.index.as_ref().unwrap();
        let query_point = EmbeddingPoint(query.clone());

        let mut search = Search::default();
        let results = index.search(&query_point, &mut search);

        let mut filtered_results = Vec::new();

        for item in results.take(k * 10) {
            // Take extra to account for filtering
            let node_id = *item.value;
            let distance = item.distance;

            // Apply filter
            if let Some(f) = filter {
                if !self.matches_filter(&node_id, f) {
                    continue;
                }
            }

            filtered_results.push(SimilarityResult {
                node_id,
                score: Self::distance_to_similarity(distance),
                distance,
            });

            if filtered_results.len() >= k {
                break;
            }
        }

        Ok(filtered_results)
    }

    fn search_threshold(
        &self,
        query: &Embedding,
        threshold: f32,
        filter: Option<&VectorFilter>,
    ) -> Result<Vec<SimilarityResult>> {
        let results = self.search(query, self.vectors.len().max(1), filter)?;

        Ok(results
            .into_iter()
            .filter(|r| r.score >= threshold)
            .collect())
    }

    fn search_batch(
        &self,
        queries: &[(NodeId, Embedding)],
        k: usize,
        filter: Option<&VectorFilter>,
    ) -> Result<HashMap<NodeId, Vec<SimilarityResult>>> {
        // Parallel batch search using rayon
        let results: Vec<(NodeId, Result<Vec<SimilarityResult>>)> = queries
            .par_iter()
            .map(|(query_id, embedding)| {
                let search_results = self.search(embedding, k, filter);
                (*query_id, search_results)
            })
            .collect();

        let mut map = HashMap::with_capacity(results.len());
        for (id, result) in results {
            map.insert(id, result?);
        }
        Ok(map)
    }

    fn len(&self) -> usize {
        self.vectors.len()
    }

    fn rebuild(&mut self) -> Result<()> {
        if self.vectors.is_empty() {
            self.index = None;
            return Ok(());
        }

        let mut points = Vec::new();
        let mut values = Vec::new();

        for (id, vec) in &self.vectors {
            points.push(EmbeddingPoint(vec.clone()));
            values.push(*id);
        }

        let map = Builder::default().build(points, values);

        self.index = Some(map);

        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(&(&self.vectors, &self.metadata, self.dimension))
            .map_err(|e| CortexError::Validation(format!("Failed to serialize index: {}", e)))?;

        fs::write(path, data)
            .map_err(|e| CortexError::Validation(format!("Failed to write index file: {}", e)))?;

        Ok(())
    }

    fn load(path: &Path) -> Result<Self>
    where
        Self: Sized,
    {
        let data = fs::read(path)
            .map_err(|e| CortexError::Validation(format!("Failed to read index file: {}", e)))?;

        let (vectors, metadata, dimension): (
            HashMap<NodeId, Vec<f32>>,
            HashMap<NodeId, NodeMetadata>,
            usize,
        ) = bincode::deserialize(&data)
            .map_err(|e| CortexError::Validation(format!("Failed to deserialize index: {}", e)))?;

        let mut index = Self {
            index: None,
            vectors,
            metadata,
            dimension,
        };

        // Rebuild the HNSW structure
        index.rebuild()?;

        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_embedding(values: Vec<f32>) -> Embedding {
        values
    }

    #[test]
    fn test_index_insert_and_search() {
        let mut index = HnswIndex::new(3);

        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();
        let id3 = NodeId::now_v7();

        index
            .insert(id1, &create_test_embedding(vec![1.0, 0.0, 0.0]))
            .unwrap();
        index
            .insert(id2, &create_test_embedding(vec![0.9, 0.1, 0.0]))
            .unwrap();
        index
            .insert(id3, &create_test_embedding(vec![0.0, 1.0, 0.0]))
            .unwrap();

        index.rebuild().unwrap();

        // Search for something close to [1.0, 0.0, 0.0]
        let results = index
            .search(&create_test_embedding(vec![1.0, 0.0, 0.0]), 2, None)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].node_id, id1);
    }

    #[test]
    fn test_threshold_search() {
        let mut index = HnswIndex::new(3);

        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index
            .insert(id1, &create_test_embedding(vec![1.0, 0.0, 0.0]))
            .unwrap();
        index
            .insert(id2, &create_test_embedding(vec![0.0, 1.0, 0.0]))
            .unwrap();

        index.rebuild().unwrap();

        // High threshold should only return very similar vectors
        let results = index
            .search_threshold(&create_test_embedding(vec![1.0, 0.0, 0.0]), 0.95, None)
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, id1);
    }

    #[test]
    fn test_index_persistence() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test.hnsw");

        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();

        index
            .insert(id1, &create_test_embedding(vec![1.0, 0.0, 0.0]))
            .unwrap();
        index.rebuild().unwrap();

        // Save
        index.save(&index_path).unwrap();

        // Load
        let loaded_index = HnswIndex::load(&index_path).unwrap();

        assert_eq!(loaded_index.len(), 1);

        // Search should work on loaded index
        let results = loaded_index
            .search(&create_test_embedding(vec![1.0, 0.0, 0.0]), 1, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, id1);
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    #[allow(dead_code)]
    fn make_embedding(dim: usize, val: f32) -> Embedding {
        vec![val; dim]
    }

    #[test]
    fn test_dimension_mismatch_rejected() {
        let mut index = HnswIndex::new(3);
        let id = NodeId::now_v7();
        assert!(index.insert(id, &vec![1.0, 2.0]).is_err());
    }

    #[test]
    fn test_empty_index_search() {
        let index = HnswIndex::new(3);
        let results = index.search(&vec![1.0, 0.0, 0.0], 5, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_brute_force_fallback() {
        // Insert without rebuild — should use brute force
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id2, &vec![0.0, 1.0, 0.0]).unwrap();
        // Don't call rebuild!

        let results = index.search(&vec![1.0, 0.0, 0.0], 2, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].node_id, id1); // Most similar first
    }

    #[test]
    fn test_filter_by_kind() {
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.set_metadata(id1, NodeKind::new("fact").unwrap(), "test".into());
        index.insert(id2, &vec![0.9, 0.1, 0.0]).unwrap();
        index.set_metadata(id2, NodeKind::new("decision").unwrap(), "test".into());
        index.rebuild().unwrap();

        let filter = VectorFilter::new().with_kinds(vec![NodeKind::new("decision").unwrap()]);
        let results = index
            .search(&vec![1.0, 0.0, 0.0], 5, Some(&filter))
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, id2);
    }

    #[test]
    fn test_filter_exclude() {
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id2, &vec![0.9, 0.1, 0.0]).unwrap();
        index.rebuild().unwrap();

        let filter = VectorFilter::new().excluding(vec![id1]);
        let results = index
            .search(&vec![1.0, 0.0, 0.0], 5, Some(&filter))
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, id2);
    }

    #[test]
    fn test_remove_doesnt_crash_search() {
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id2, &vec![0.0, 1.0, 0.0]).unwrap();
        index.rebuild().unwrap();

        index.remove(id1).unwrap();
        assert_eq!(index.len(), 1);

        // Search still works (may return stale results until rebuild)
        let results = index.search(&vec![1.0, 0.0, 0.0], 5, None).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_search_batch() {
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();
        let id3 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id2, &vec![0.0, 1.0, 0.0]).unwrap();
        index.insert(id3, &vec![0.0, 0.0, 1.0]).unwrap();
        index.rebuild().unwrap();

        let queries = vec![(id1, vec![1.0, 0.0, 0.0]), (id2, vec![0.0, 1.0, 0.0])];
        let results = index.search_batch(&queries, 1, None).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[&id1][0].node_id, id1);
        assert_eq!(results[&id2][0].node_id, id2);
    }

    #[test]
    fn test_similarity_score_range() {
        let mut index = HnswIndex::new(3);
        let id1 = NodeId::now_v7();
        let id2 = NodeId::now_v7();

        index.insert(id1, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id2, &vec![-1.0, 0.0, 0.0]).unwrap(); // Opposite direction
        index.rebuild().unwrap();

        let results = index.search(&vec![1.0, 0.0, 0.0], 2, None).unwrap();

        // All scores should be in [0.0, 1.0]
        for r in &results {
            assert!(
                r.score >= 0.0 && r.score <= 1.0,
                "Score {} out of range",
                r.score
            );
        }
        // First result (same vector) should have score ~1.0
        assert!(results[0].score > 0.99);
    }

    #[test]
    fn test_threshold_returns_only_above() {
        let mut index = HnswIndex::new(3);

        let id_close = NodeId::now_v7();
        let id_far = NodeId::now_v7();

        index.insert(id_close, &vec![1.0, 0.0, 0.0]).unwrap();
        index.insert(id_far, &vec![0.0, 0.0, 1.0]).unwrap();
        index.rebuild().unwrap();

        let results = index
            .search_threshold(&vec![1.0, 0.0, 0.0], 0.5, None)
            .unwrap();

        // Only the close vector should be above threshold
        assert!(results.iter().all(|r| r.score >= 0.5));
        assert!(results.iter().any(|r| r.node_id == id_close));
    }
}
