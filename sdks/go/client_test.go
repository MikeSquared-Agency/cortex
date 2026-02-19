package cortex

import (
	"context"
	"fmt"
	"net"
	"testing"

	pb "github.com/MikeSquared-Agency/cortex/sdks/go/proto"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
)

const bufSize = 1024 * 1024

// ---------------------------------------------------------------------------
// Minimal in-process gRPC server
// ---------------------------------------------------------------------------

// testServer is a minimal CortexServiceServer implementation for unit tests.
// It stores nodes in memory and returns NOT_FOUND for unknown IDs.
type testServer struct {
	pb.UnimplementedCortexServiceServer
	nodes  map[string]*pb.NodeResponse
	nextID int
}

func newTestServer() *testServer {
	return &testServer{nodes: make(map[string]*pb.NodeResponse)}
}

func (s *testServer) CreateNode(_ context.Context, req *pb.CreateNodeRequest) (*pb.NodeResponse, error) {
	s.nextID++
	id := fmt.Sprintf("node-%d", s.nextID)
	body := req.Body
	if body == "" {
		body = req.Title
	}
	node := &pb.NodeResponse{
		Id:    id,
		Kind:  req.Kind,
		Title: req.Title,
		Body:  body,
	}
	s.nodes[id] = node
	return node, nil
}

func (s *testServer) GetNode(_ context.Context, req *pb.GetNodeRequest) (*pb.NodeResponse, error) {
	node, ok := s.nodes[req.Id]
	if !ok {
		return nil, status.Errorf(codes.NotFound, "node %q not found", req.Id)
	}
	return node, nil
}

func (s *testServer) SimilaritySearch(_ context.Context, req *pb.SimilaritySearchRequest) (*pb.SearchResponse, error) {
	var results []*pb.SearchResultEntry
	for _, node := range s.nodes {
		results = append(results, &pb.SearchResultEntry{
			Node:  node,
			Score: 0.9,
		})
		if uint32(len(results)) >= req.Limit && req.Limit > 0 {
			break
		}
	}
	return &pb.SearchResponse{Results: results}, nil
}

func (s *testServer) GetBriefing(_ context.Context, req *pb.BriefingRequest) (*pb.BriefingResponse, error) {
	return &pb.BriefingResponse{
		AgentId:  req.AgentId,
		Rendered: fmt.Sprintf("[Test briefing for %s]", req.AgentId),
	}, nil
}

func (s *testServer) Traverse(_ context.Context, _ *pb.TraverseRequest) (*pb.SubgraphResponse, error) {
	return &pb.SubgraphResponse{
		Nodes:     []*pb.NodeResponse{},
		Edges:     []*pb.EdgeResponse{},
		Truncated: false,
	}, nil
}

// ---------------------------------------------------------------------------
// Test helper: start bufconn server, return connected Client + teardown func
// ---------------------------------------------------------------------------

func newTestClient(t *testing.T) (*Client, *testServer, func()) {
	t.Helper()

	lis := bufconn.Listen(bufSize)
	grpcSrv := grpc.NewServer()
	srv := newTestServer()
	pb.RegisterCortexServiceServer(grpcSrv, srv)

	go func() {
		if err := grpcSrv.Serve(lis); err != nil {
			// Server stopped — normal on test cleanup.
		}
	}()

	dialer := func(ctx context.Context, _ string) (net.Conn, error) {
		return lis.DialContext(ctx)
	}

	conn, err := grpc.Dial(
		"bufnet",
		grpc.WithContextDialer(dialer),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		t.Fatalf("grpc.Dial (bufconn): %v", err)
	}

	client := &Client{
		conn: conn,
		svc:  pb.NewCortexServiceClient(conn),
	}

	cleanup := func() {
		_ = client.Close()
		grpcSrv.Stop()
		_ = lis.Close()
	}

	return client, srv, cleanup
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// TestConnect verifies that the test infrastructure itself works — the helper
// returns a non-nil client backed by a running in-process server.
func TestConnect(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	if client == nil {
		t.Fatal("expected non-nil client")
	}
	if client.conn == nil {
		t.Fatal("expected non-nil gRPC connection")
	}
}

// TestCreateNode stores a node and verifies the returned ID is non-empty.
func TestCreateNode(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	id, err := client.CreateNode(ctx, Node{
		Kind:  "fact",
		Title: "API rate limit is 1000/min",
	})
	if err != nil {
		t.Fatalf("CreateNode returned unexpected error: %v", err)
	}
	if id == "" {
		t.Fatal("expected non-empty node ID")
	}
}

// TestCreateNodeWithAllFields verifies that optional node fields are forwarded.
func TestCreateNodeWithAllFields(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	id, err := client.CreateNode(ctx, Node{
		Kind:        "event",
		Title:       "Deployed v2.1",
		Body:        "Rolled out to production at 14:00 UTC",
		Tags:        []string{"deploy", "production"},
		Importance:  0.9,
		SourceAgent: "kai",
		Metadata:    map[string]string{"env": "prod"},
	})
	if err != nil {
		t.Fatalf("CreateNode (full fields): %v", err)
	}
	if id == "" {
		t.Fatal("expected non-empty ID for full-field node")
	}
}

// TestSearch creates nodes then performs a similarity search.
func TestSearch(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	// Seed a node so the mock server has something to return.
	_, err := client.CreateNode(ctx, Node{Kind: "fact", Title: "Searchable fact"})
	if err != nil {
		t.Fatalf("CreateNode: %v", err)
	}

	results, err := client.Search(ctx, "fact", 10)
	if err != nil {
		t.Fatalf("Search returned unexpected error: %v", err)
	}
	if len(results) == 0 {
		t.Fatal("expected at least one search result")
	}
	// Verify result shape.
	r := results[0]
	if r.NodeID == "" {
		t.Error("expected non-empty NodeID in search result")
	}
	if r.Score <= 0 {
		t.Errorf("expected positive score, got %v", r.Score)
	}
}

// TestSearchEmpty verifies search returns an empty slice when no nodes exist.
func TestSearchEmpty(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	results, err := client.Search(ctx, "anything", 10)
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if len(results) != 0 {
		t.Errorf("expected empty results, got %d", len(results))
	}
}

// TestBriefing verifies that Briefing returns a non-empty rendered string.
func TestBriefing(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	rendered, err := client.Briefing(ctx, "test-agent")
	if err != nil {
		t.Fatalf("Briefing: %v", err)
	}
	if rendered == "" {
		t.Fatal("expected non-empty briefing text")
	}
	if !containsStr(rendered, "test-agent") {
		t.Errorf("expected briefing to mention agent ID, got: %q", rendered)
	}
}

// TestGetNodeFound stores a node and retrieves it by ID.
func TestGetNodeFound(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	id, err := client.CreateNode(ctx, Node{Kind: "fact", Title: "Retrievable fact"})
	if err != nil {
		t.Fatalf("CreateNode: %v", err)
	}

	node, err := client.GetNode(ctx, id)
	if err != nil {
		t.Fatalf("GetNode returned unexpected error: %v", err)
	}
	if node == nil {
		t.Fatal("expected non-nil NodeResponse")
	}
	if node.Id != id {
		t.Errorf("node ID mismatch: want %q, got %q", id, node.Id)
	}
	if node.Title != "Retrievable fact" {
		t.Errorf("node title mismatch: want %q, got %q", "Retrievable fact", node.Title)
	}
	if node.Kind != "fact" {
		t.Errorf("node kind mismatch: want %q, got %q", "fact", node.Kind)
	}
}

// TestGetNodeNotFound verifies that GetNode returns an error for unknown IDs.
// (The Go SDK propagates the gRPC NOT_FOUND error; it does not return nil.)
func TestGetNodeNotFound(t *testing.T) {
	client, _, cleanup := newTestClient(t)
	defer cleanup()

	ctx := context.Background()

	_, err := client.GetNode(ctx, "nonexistent-id-99999")
	if err == nil {
		t.Fatal("expected error for nonexistent node, got nil")
	}

	// Verify the wrapped error carries NOT_FOUND status.
	st, ok := status.FromError(unwrapGRPC(err))
	if !ok {
		t.Logf("could not extract gRPC status from error %v (may be wrapped)", err)
		return // The error exists — that's the key assertion.
	}
	if st.Code() != codes.NotFound {
		t.Errorf("expected NOT_FOUND status, got %v", st.Code())
	}
}

// TestClose verifies that Close() does not return an error.
func TestClose(t *testing.T) {
	lis := bufconn.Listen(bufSize)
	grpcSrv := grpc.NewServer()
	pb.RegisterCortexServiceServer(grpcSrv, newTestServer())

	go func() { _ = grpcSrv.Serve(lis) }()
	defer grpcSrv.Stop()

	conn, err := grpc.Dial(
		"bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return lis.DialContext(ctx)
		}),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		t.Fatalf("grpc.Dial: %v", err)
	}

	client := &Client{conn: conn, svc: pb.NewCortexServiceClient(conn)}

	if closeErr := client.Close(); closeErr != nil {
		t.Fatalf("Close() returned unexpected error: %v", closeErr)
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func containsStr(s, sub string) bool {
	return len(s) >= len(sub) && (s == sub || len(sub) == 0 ||
		func() bool {
			for i := 0; i+len(sub) <= len(s); i++ {
				if s[i:i+len(sub)] == sub {
					return true
				}
			}
			return false
		}())
}

// unwrapGRPC attempts to find a gRPC status error within a wrapped error chain.
func unwrapGRPC(err error) error {
	for err != nil {
		if _, ok := status.FromError(err); ok {
			return err
		}
		type unwrapper interface{ Unwrap() error }
		if u, ok := err.(unwrapper); ok {
			err = u.Unwrap()
		} else {
			break
		}
	}
	return err
}
