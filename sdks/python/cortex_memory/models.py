"""Plain data models for Cortex SDK responses."""
from __future__ import annotations
from typing import List, Optional


class Node:
    """A knowledge node retrieved from Cortex."""

    def __init__(self, proto) -> None:
        self.id: str = proto.id
        self.kind: str = proto.kind
        self.title: str = proto.title
        self.body: str = proto.body
        self.importance: float = proto.importance
        self.tags: List[str] = list(proto.tags)
        self.source_agent: str = proto.source_agent

    def __repr__(self) -> str:
        return f"Node(id={self.id!r}, kind={self.kind!r}, title={self.title!r})"


class Edge:
    """A directed edge between two nodes."""

    def __init__(self, proto) -> None:
        self.id: str = proto.id
        self.from_id: str = proto.from_id
        self.to_id: str = proto.to_id
        self.relation: str = proto.relation
        self.weight: float = proto.weight

    def __repr__(self) -> str:
        return f"Edge({self.from_id!r} -[{self.relation}]-> {self.to_id!r})"


class SearchResult:
    """A single similarity search hit."""

    def __init__(self, proto) -> None:
        self.score: float = proto.score
        # SearchResultEntry wraps a NodeResponse in proto.node
        node = proto.node
        self.node_id: str = node.id
        self.title: str = node.title
        self.kind: str = node.kind
        self.body: str = node.body
        self.importance: float = node.importance

    def __repr__(self) -> str:
        return f"SearchResult(score={self.score:.3f}, kind={self.kind!r}, title={self.title!r})"


class Briefing:
    """A rendered context briefing for an agent."""

    def __init__(self, proto) -> None:
        self.agent_id: str = proto.agent_id
        self.text: str = proto.rendered
        self.nodes_consulted: int = proto.nodes_consulted
        self.cached: bool = proto.cached
        self.generated_at: str = proto.generated_at

    def __str__(self) -> str:
        return self.text

    def __repr__(self) -> str:
        return (
            f"Briefing(agent={self.agent_id!r}, nodes={self.nodes_consulted}, "
            f"cached={self.cached})"
        )
