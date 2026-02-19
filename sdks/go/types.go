// Package cortex provides a Go client for the Cortex graph memory engine.
package cortex

// Node holds the fields for creating or describing a knowledge node.
type Node struct {
	Kind       string
	Title      string
	Body       string
	Tags       []string
	Importance float32
	SourceAgent string
	Metadata   map[string]string
}

// SearchResult represents a single similarity search hit.
type SearchResult struct {
	Score      float32
	NodeID     string
	Title      string
	Kind       string
	Body       string
	Importance float32
}

// HybridResult represents a hit from hybrid (vector + graph) search.
type HybridResult struct {
	CombinedScore float32
	VectorScore   float32
	GraphScore    float32
	NodeID        string
	Title         string
	Kind          string
}

// Subgraph holds nodes and edges returned by a traversal.
type Subgraph struct {
	// NodeIDs lists the IDs of nodes in the subgraph.
	// Use the raw proto response for full node data.
	NodeIDs   []string
	EdgeCount int
	Truncated bool
}
