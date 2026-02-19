package cortex

import (
	"context"
	"fmt"

	pb "github.com/MikeSquared-Agency/cortex/sdks/go/proto"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// Client is a connected Cortex client.
//
// Example:
//
//	client, err := cortex.Connect("localhost:9090")
//	if err != nil { panic(err) }
//	defer client.Close()
//
//	ctx := context.Background()
//	id, _ := client.CreateNode(ctx, cortex.Node{Kind: "event", Title: "Deployed v2.1"})
//	fmt.Println("Created:", id)
type Client struct {
	conn *grpc.ClientConn
	svc  pb.CortexServiceClient
}

// Connect creates a new Client connected to addr (e.g. "localhost:9090").
func Connect(addr string) (*Client, error) {
	conn, err := grpc.Dial(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("cortex: connect: %w", err)
	}
	return &Client{conn: conn, svc: pb.NewCortexServiceClient(conn)}, nil
}

// Close releases the underlying gRPC connection.
func (c *Client) Close() error {
	return c.conn.Close()
}

// CreateNode stores a new knowledge node and returns its ID.
func (c *Client) CreateNode(ctx context.Context, n Node) (string, error) {
	req := &pb.CreateNodeRequest{
		Kind:        n.Kind,
		Title:       n.Title,
		Body:        orDefault(n.Body, n.Title),
		Importance:  n.Importance,
		Tags:        n.Tags,
		SourceAgent: n.SourceAgent,
	}
	if n.Metadata != nil {
		req.Metadata = n.Metadata
	}
	resp, err := c.svc.CreateNode(ctx, req)
	if err != nil {
		return "", fmt.Errorf("cortex: CreateNode: %w", err)
	}
	return resp.Id, nil
}

// GetNode retrieves a node by ID. Returns nil if not found.
func (c *Client) GetNode(ctx context.Context, id string) (*pb.NodeResponse, error) {
	resp, err := c.svc.GetNode(ctx, &pb.GetNodeRequest{Id: id})
	if err != nil {
		return nil, fmt.Errorf("cortex: GetNode: %w", err)
	}
	return resp, nil
}

// Search performs semantic similarity search and returns ranked results.
func (c *Client) Search(ctx context.Context, query string, limit int) ([]SearchResult, error) {
	resp, err := c.svc.SimilaritySearch(ctx, &pb.SimilaritySearchRequest{
		Query: query,
		Limit: uint32(limit),
	})
	if err != nil {
		return nil, fmt.Errorf("cortex: Search: %w", err)
	}
	results := make([]SearchResult, len(resp.Results))
	for i, r := range resp.Results {
		n := r.Node
		if n == nil {
			continue
		}
		results[i] = SearchResult{
			Score:      r.Score,
			NodeID:     n.Id,
			Title:      n.Title,
			Kind:       n.Kind,
			Body:       n.Body,
			Importance: n.Importance,
		}
	}
	return results, nil
}

// SearchHybrid performs hybrid search combining vector similarity with graph
// proximity. anchorIDs are node IDs that anchor the graph component.
func (c *Client) SearchHybrid(
	ctx context.Context,
	query string,
	anchorIDs []string,
	limit int,
) ([]HybridResult, error) {
	resp, err := c.svc.HybridSearch(ctx, &pb.HybridSearchRequest{
		Query:     query,
		AnchorIds: anchorIDs,
		Limit:     uint32(limit),
	})
	if err != nil {
		return nil, fmt.Errorf("cortex: SearchHybrid: %w", err)
	}
	results := make([]HybridResult, len(resp.Results))
	for i, r := range resp.Results {
		n := r.Node
		if n == nil {
			continue
		}
		results[i] = HybridResult{
			CombinedScore: r.CombinedScore,
			VectorScore:   r.VectorScore,
			GraphScore:    r.GraphScore,
			NodeID:        n.Id,
			Title:         n.Title,
			Kind:          n.Kind,
		}
	}
	return results, nil
}

// Briefing generates a rendered context briefing for agentID.
// Returns the markdown text. Use compact=true for denser output.
func (c *Client) Briefing(ctx context.Context, agentID string) (string, error) {
	resp, err := c.svc.GetBriefing(ctx, &pb.BriefingRequest{AgentId: agentID})
	if err != nil {
		return "", fmt.Errorf("cortex: Briefing: %w", err)
	}
	return resp.Rendered, nil
}

// BriefingCompact generates a compact briefing (denser, ~4× shorter).
func (c *Client) BriefingCompact(ctx context.Context, agentID string) (string, error) {
	resp, err := c.svc.GetBriefing(ctx, &pb.BriefingRequest{AgentId: agentID, Compact: true})
	if err != nil {
		return "", fmt.Errorf("cortex: BriefingCompact: %w", err)
	}
	return resp.Rendered, nil
}

// Traverse performs a graph traversal from startID up to depth hops.
func (c *Client) Traverse(ctx context.Context, startID string, depth uint32) (*Subgraph, error) {
	resp, err := c.svc.Traverse(ctx, &pb.TraverseRequest{
		StartIds: []string{startID},
		MaxDepth: depth,
	})
	if err != nil {
		return nil, fmt.Errorf("cortex: Traverse: %w", err)
	}
	ids := make([]string, len(resp.Nodes))
	for i, n := range resp.Nodes {
		ids[i] = n.Id
	}
	return &Subgraph{
		NodeIDs:   ids,
		EdgeCount: len(resp.Edges),
		Truncated: resp.Truncated,
	}, nil
}

func orDefault(s, d string) string {
	if s == "" {
		return d
	}
	return s
}
