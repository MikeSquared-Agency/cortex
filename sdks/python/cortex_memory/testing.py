"""
Testing utilities for the Cortex Python SDK.

Provides an in-memory ``MockCortex`` that matches the ``Cortex`` interface
without requiring a running server or gRPC, and a ``mock_cortex()`` context
manager for use in unit tests.

Usage::

    from cortex_memory.testing import mock_cortex

    def test_my_agent():
        with mock_cortex() as cx:
            cx.store("fact", "test data")
            results = cx.search("test")
            assert len(results) == 1

Or with pytest fixtures::

    import pytest
    from cortex_memory.testing import mock_cortex

    @pytest.fixture
    def cortex():
        with mock_cortex() as cx:
            yield cx
"""
from __future__ import annotations

import uuid
from contextlib import contextmanager
from typing import Dict, Generator, List, Optional


@contextmanager
def mock_cortex() -> Generator["MockCortex", None, None]:
    """
    Context manager that returns a ``Cortex``-compatible in-memory mock.

    No server required. Suitable for unit tests.
    """
    yield MockCortex()


class _MockNode:
    """Minimal node-like object returned by MockCortex."""

    def __init__(self, data: dict) -> None:
        self.__dict__.update(data)

    def __repr__(self) -> str:
        return f"MockNode(id={self.id!r}, kind={self.kind!r}, title={self.title!r})"


class _MockSearchResult:
    """Minimal search result returned by MockCortex."""

    def __init__(self, score: float, node: _MockNode) -> None:
        self.score = score
        self.node_id = node.id
        self.title = node.title
        self.kind = node.kind
        self.body = getattr(node, "body", "")
        self.importance = getattr(node, "importance", 0.5)

    def __repr__(self) -> str:
        return (
            f"MockSearchResult(score={self.score:.2f}, "
            f"kind={self.kind!r}, title={self.title!r})"
        )


class MockCortex:
    """
    In-memory Cortex implementation for testing.

    Implements the same interface as :class:`~cortex_memory.Cortex` but stores
    everything in a plain Python dict and performs simple substring matching
    for searches.
    """

    def __init__(self) -> None:
        self._nodes: Dict[str, dict] = {}
        self._call_log: List[tuple] = []

    # ------------------------------------------------------------------
    # Write
    # ------------------------------------------------------------------

    def store(
        self,
        kind: str,
        title: str,
        *,
        body: str = "",
        tags: Optional[List[str]] = None,
        importance: float = 0.5,
        metadata: Optional[dict] = None,
        source_agent: str = "",
    ) -> str:
        node_id = str(uuid.uuid4())
        self._nodes[node_id] = {
            "id": node_id,
            "kind": kind,
            "title": title,
            "body": body or title,
            "tags": tags or [],
            "importance": importance,
            "metadata": metadata or {},
            "source_agent": source_agent,
        }
        self._call_log.append(("store", kind, title))
        return node_id

    # ------------------------------------------------------------------
    # Read / search
    # ------------------------------------------------------------------

    def search(self, query: str, *, limit: int = 10) -> List[_MockSearchResult]:
        """Simple substring match on title and body."""
        q = query.lower()
        matches = [
            _MockSearchResult(0.9, _MockNode(n))
            for n in self._nodes.values()
            if q in n["title"].lower() or q in n["body"].lower()
        ]
        return matches[:limit]

    def search_hybrid(
        self,
        query: str,
        *,
        anchor_ids: Optional[List[str]] = None,
        limit: int = 10,
    ) -> List[_MockSearchResult]:
        return self.search(query, limit=limit)

    def briefing(self, agent_id: str, *, compact: bool = False) -> str:
        return f"[Mock briefing for {agent_id}]"

    def briefing_full(self, agent_id: str, *, compact: bool = False):
        class _MockBriefing:
            text = f"[Mock briefing for {agent_id}]"
            nodes_consulted = len(self._nodes)
            cached = False
            generated_at = ""

            def __str__(self_):
                return self_.text

        return _MockBriefing()

    def get_node(self, node_id: str) -> Optional[_MockNode]:
        data = self._nodes.get(node_id)
        return _MockNode(data) if data else None

    def traverse(
        self,
        node_id: str,
        *,
        depth: int = 2,
        direction: str = "both",
    ) -> dict:
        return {"nodes": [], "edges": []}

    # ------------------------------------------------------------------
    # Assertion helpers
    # ------------------------------------------------------------------

    def assert_stored(self, kind: str, title: str) -> None:
        """Assert that ``store(kind, title)`` was called."""
        for entry in self._call_log:
            if entry[0] == "store" and entry[1] == kind and entry[2] == title:
                return
        raise AssertionError(
            f"Expected store({kind!r}, {title!r}) but it was not called.\n"
            f"Calls: {self._call_log}"
        )

    def assert_not_stored(self, kind: str, title: str) -> None:
        """Assert that ``store(kind, title)`` was NOT called."""
        for entry in self._call_log:
            if entry[0] == "store" and entry[1] == kind and entry[2] == title:
                raise AssertionError(
                    f"Expected store({kind!r}, {title!r}) NOT to be called, but it was."
                )

    # ------------------------------------------------------------------
    # Context manager / lifecycle
    # ------------------------------------------------------------------

    def close(self) -> None:
        pass

    def __enter__(self) -> "MockCortex":
        return self

    def __exit__(self, *_) -> None:
        self.close()
