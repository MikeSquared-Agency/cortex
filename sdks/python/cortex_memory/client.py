"""Cortex gRPC client — connects to a running Cortex server."""
from __future__ import annotations

from typing import Dict, List, Optional

import grpc

from . import cortex_pb2, cortex_pb2_grpc
from .models import Briefing, Node, SearchResult


class Cortex:
    """
    Cortex client — connects to a running Cortex server over gRPC.

    Server mode::

        cx = Cortex("localhost:9090")

    Library mode (starts embedded server subprocess)::

        cx = Cortex.open("./memory.redb")

    Always use as a context manager or call ``close()`` when done::

        with Cortex("localhost:9090") as cx:
            cx.store("fact", "The sky is blue")
    """

    def __init__(self, addr: str) -> None:
        """Connect to a running Cortex gRPC server at *addr* (e.g. ``"localhost:9090"``)."""
        self._channel = grpc.insecure_channel(addr)
        self._stub = cortex_pb2_grpc.CortexServiceStub(self._channel)
        self._proc = None

    @classmethod
    def open(cls, path: str) -> "Cortex":
        """
        Library mode: open an embedded database without a running server.

        Requires the ``cortex`` binary on PATH (provided by Phase 7B CLI).
        Starts a local server subprocess on a random port and returns a
        connected client. The subprocess is terminated when ``close()`` is called.
        """
        import subprocess
        import time

        port = _find_free_port()
        proc = subprocess.Popen(
            ["cortex", "serve", "--db", path, "--grpc-addr", f"127.0.0.1:{port}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        # Wait for server to become ready (up to 5 seconds)
        target = f"127.0.0.1:{port}"
        for _ in range(20):
            try:
                ch = grpc.insecure_channel(target)
                grpc.channel_ready_future(ch).result(timeout=0.5)
                ch.close()
                break
            except grpc.FutureTimeoutError:
                time.sleep(0.25)

        instance = cls(target)
        instance._proc = proc
        return instance

    # ------------------------------------------------------------------
    # Core write operations
    # ------------------------------------------------------------------

    def store(
        self,
        kind: str,
        title: str,
        *,
        body: str = "",
        tags: Optional[List[str]] = None,
        importance: float = 0.5,
        metadata: Optional[Dict[str, str]] = None,
        source_agent: str = "",
    ) -> str:
        """Store a knowledge node. Returns the node ID string."""
        req = cortex_pb2.CreateNodeRequest(
            kind=kind,
            title=title,
            body=body or title,
            importance=importance,
            tags=tags or [],
            source_agent=source_agent,
        )
        if metadata:
            req.metadata.update(metadata)
        resp = self._stub.CreateNode(req)
        return resp.id

    # ------------------------------------------------------------------
    # Read / search operations
    # ------------------------------------------------------------------

    def search(self, query: str, *, limit: int = 10) -> List[SearchResult]:
        """Semantic similarity search. Returns ranked ``SearchResult`` objects."""
        resp = self._stub.SimilaritySearch(
            cortex_pb2.SimilaritySearchRequest(query=query, limit=limit)
        )
        return [SearchResult(r) for r in resp.results]

    def search_hybrid(
        self,
        query: str,
        *,
        anchor_ids: Optional[List[str]] = None,
        limit: int = 10,
    ) -> List[SearchResult]:
        """
        Hybrid search combining vector similarity with graph proximity.

        ``anchor_ids`` are node IDs that anchor the graph component.
        """
        resp = self._stub.HybridSearch(
            cortex_pb2.HybridSearchRequest(
                query=query,
                anchor_ids=anchor_ids or [],
                limit=limit,
            )
        )
        # HybridResultEntry has node + combined_score — adapt to SearchResult
        return [_hybrid_to_search_result(r) for r in resp.results]

    def briefing(self, agent_id: str, *, compact: bool = False) -> str:
        """
        Generate a context briefing for an agent.

        Returns rendered markdown text ready to inject into a system prompt.
        Use ``compact=True`` for a denser (~4× shorter) format.
        """
        resp = self._stub.GetBriefing(
            cortex_pb2.BriefingRequest(agent_id=agent_id, compact=compact)
        )
        return resp.rendered

    def briefing_full(self, agent_id: str, *, compact: bool = False) -> Briefing:
        """Like ``briefing()`` but returns a :class:`Briefing` with metadata."""
        resp = self._stub.GetBriefing(
            cortex_pb2.BriefingRequest(agent_id=agent_id, compact=compact)
        )
        return Briefing(resp)

    def get_node(self, node_id: str) -> Optional[Node]:
        """Get a node by ID. Returns ``None`` if not found."""
        try:
            resp = self._stub.GetNode(cortex_pb2.GetNodeRequest(id=node_id))
            return Node(resp)
        except grpc.RpcError as e:
            if e.code() == grpc.StatusCode.NOT_FOUND:
                return None
            raise

    def traverse(
        self,
        node_id: str,
        *,
        depth: int = 2,
        direction: str = "both",
    ) -> dict:
        """
        Graph traversal from *node_id*.

        Returns ``{"nodes": [NodeResponse, ...], "edges": [EdgeResponse, ...]}``.
        ``direction`` is ``"outgoing"``, ``"incoming"``, or ``"both"``.
        """
        resp = self._stub.Traverse(
            cortex_pb2.TraverseRequest(
                start_ids=[node_id],
                max_depth=depth,
                direction=direction,
            )
        )
        return {"nodes": list(resp.nodes), "edges": list(resp.edges)}

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def close(self) -> None:
        """Close the gRPC channel and terminate any subprocess started by ``open()``."""
        self._channel.close()
        if self._proc is not None:
            self._proc.terminate()
            self._proc = None

    def __enter__(self) -> "Cortex":
        return self

    def __exit__(self, *_) -> None:
        self.close()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _hybrid_to_search_result(entry):
    """Adapt a HybridResultEntry to the same interface as SearchResult."""

    class _Adapter:
        def __init__(self, e):
            self.score = e.combined_score
            self.node = e.node

    return SearchResult(_Adapter(entry))


def _find_free_port() -> int:
    import socket

    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]
