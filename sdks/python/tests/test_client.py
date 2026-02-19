"""
Unit tests for the Cortex Python SDK.

All tests use MockCortex via the mock_cortex fixture — no real gRPC server
or network connection is required.
"""
import pytest
from cortex_memory.testing import MockCortex, mock_cortex


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def cx() -> MockCortex:
    """Provide a fresh MockCortex for each test."""
    with mock_cortex() as m:
        yield m


# ---------------------------------------------------------------------------
# Constructor
# ---------------------------------------------------------------------------

class TestConstructor:
    def test_creates_empty_node_store(self):
        mc = MockCortex()
        assert len(mc._nodes) == 0

    def test_creates_empty_call_log(self):
        mc = MockCortex()
        assert len(mc._call_log) == 0

    def test_multiple_instances_are_independent(self):
        a = MockCortex()
        b = MockCortex()
        a.store("fact", "Only in A")
        assert len(a._nodes) == 1
        assert len(b._nodes) == 0

    def test_mock_cortex_context_manager_yields_instance(self):
        with mock_cortex() as cx:
            assert isinstance(cx, MockCortex)


# ---------------------------------------------------------------------------
# store()
# ---------------------------------------------------------------------------

class TestStore:
    def test_returns_string_id(self, cx):
        node_id = cx.store("fact", "Test fact")
        assert isinstance(node_id, str)
        assert len(node_id) > 0

    def test_returns_unique_ids(self, cx):
        id1 = cx.store("fact", "Node A")
        id2 = cx.store("fact", "Node B")
        assert id1 != id2

    def test_increments_node_count(self, cx):
        cx.store("fact", "First")
        assert len(cx._nodes) == 1
        cx.store("event", "Second")
        assert len(cx._nodes) == 2

    def test_accepts_all_optional_fields(self, cx):
        node_id = cx.store(
            "note",
            "My annotated note",
            body="Extended body",
            tags=["alpha", "beta"],
            importance=0.8,
            metadata={"project": "cortex", "env": "test"},
            source_agent="kai",
        )
        assert isinstance(node_id, str)

    def test_defaults_body_to_title(self, cx):
        node_id = cx.store("fact", "Title only")
        assert cx._nodes[node_id]["body"] == "Title only"

    def test_records_call_in_log(self, cx):
        cx.store("fact", "Logged fact")
        cx.assert_stored("fact", "Logged fact")  # should not raise


# ---------------------------------------------------------------------------
# search()
# ---------------------------------------------------------------------------

class TestSearch:
    def test_returns_matching_results(self, cx):
        cx.store("fact", "Rate limit is 1000/min")
        cx.store("event", "Deploy complete")

        results = cx.search("rate limit")
        assert len(results) == 1
        assert results[0].title == "Rate limit is 1000/min"
        assert results[0].kind == "fact"
        assert results[0].score == pytest.approx(0.9)

    def test_returns_empty_list_when_no_match(self, cx):
        cx.store("fact", "Something unrelated")
        results = cx.search("zyx-no-match-xyz")
        assert results == []

    def test_matches_on_body_text(self, cx):
        cx.store("note", "Short title", body="Detailed body content here")
        results = cx.search("detailed body")
        assert len(results) == 1

    def test_search_is_case_insensitive(self, cx):
        cx.store("fact", "Important Discovery")
        results = cx.search("important discovery")
        assert len(results) == 1

    def test_respects_limit(self, cx):
        for i in range(6):
            cx.store("fact", f"Item number {i}")
        results = cx.search("Item", limit=3)
        assert len(results) <= 3

    def test_returns_all_when_limit_exceeds_matches(self, cx):
        cx.store("fact", "Just one match")
        results = cx.search("one match", limit=100)
        assert len(results) == 1


# ---------------------------------------------------------------------------
# briefing()
# ---------------------------------------------------------------------------

class TestBriefing:
    def test_returns_non_empty_string(self, cx):
        text = cx.briefing("kai")
        assert isinstance(text, str)
        assert len(text) > 0

    def test_includes_agent_id(self, cx):
        text = cx.briefing("my-agent")
        assert "my-agent" in text

    def test_compact_flag_accepted(self, cx):
        text = cx.briefing("kai", compact=True)
        assert isinstance(text, str)

    def test_different_agents_produce_different_briefings(self, cx):
        t1 = cx.briefing("agent-alpha")
        t2 = cx.briefing("agent-beta")
        assert t1 != t2


# ---------------------------------------------------------------------------
# get_node()  — found + not found
# ---------------------------------------------------------------------------

class TestGetNode:
    def test_returns_node_when_found(self, cx):
        node_id = cx.store("fact", "Findable node", importance=0.7)
        node = cx.get_node(node_id)

        assert node is not None
        assert node.id == node_id
        assert node.kind == "fact"
        assert node.title == "Findable node"
        assert node.importance == pytest.approx(0.7)

    def test_returns_none_when_not_found(self, cx):
        result = cx.get_node("nonexistent-node-id-99999")
        assert result is None

    def test_returns_none_for_empty_string(self, cx):
        result = cx.get_node("")
        assert result is None

    def test_returns_correct_node_when_multiple_stored(self, cx):
        id_a = cx.store("fact", "Node A")
        id_b = cx.store("event", "Node B")

        node_b = cx.get_node(id_b)
        assert node_b is not None
        assert node_b.title == "Node B"
        assert node_b.kind == "event"


# ---------------------------------------------------------------------------
# assert_stored() / assert_not_stored() helpers
# ---------------------------------------------------------------------------

class TestAssertStored:
    def test_passes_when_store_was_called(self, cx):
        cx.store("fact", "Known fact")
        cx.assert_stored("fact", "Known fact")  # no exception

    def test_raises_when_store_was_not_called(self, cx):
        with pytest.raises(AssertionError):
            cx.assert_stored("fact", "Never stored")

    def test_is_strict_about_kind(self, cx):
        cx.store("event", "Same title")
        with pytest.raises(AssertionError):
            cx.assert_stored("fact", "Same title")

    def test_is_strict_about_title(self, cx):
        cx.store("fact", "Actual title")
        with pytest.raises(AssertionError):
            cx.assert_stored("fact", "Different title")


class TestAssertNotStored:
    def test_passes_when_store_was_not_called(self, cx):
        cx.assert_not_stored("fact", "Never stored")  # no exception

    def test_raises_when_store_was_called(self, cx):
        cx.store("fact", "Stored fact")
        with pytest.raises(AssertionError):
            cx.assert_not_stored("fact", "Stored fact")
