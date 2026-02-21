use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ── Label types ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct KindLabel {
    pub kind: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RelationLabel {
    pub relation: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct GateCheckLabel {
    pub check: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct EndpointLabel {
    pub endpoint: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct HttpLabel {
    pub method: String,
    pub status: String,
}

// ── Metrics registry ───────────────────────────────────────────────────────────

pub struct CortexMetrics {
    pub registry: Registry,

    // Graph — gauges, updated from storage stats at scrape time
    pub nodes_by_kind: Family<KindLabel, Gauge>,
    pub edges_by_relation: Family<RelationLabel, Gauge>,
    pub node_count: Gauge,
    pub edge_count: Gauge,
    pub db_size: Gauge,

    // Auto-linker — counters (cumulative, incremented after each cycle)
    pub linker_cycles: Counter,
    pub linker_edges_created: Counter,
    pub linker_edges_pruned: Counter,
    pub linker_edges_deleted: Counter,
    pub linker_duplicates_found: Counter,
    pub linker_contradictions_found: Counter,

    // Auto-linker — gauges (current state)
    pub linker_backlog: Gauge,
    pub linker_last_cycle_nodes: Gauge,
    pub linker_last_cycle_edges: Gauge,

    // Auto-linker — cycle duration histogram
    pub linker_cycle_duration: Histogram,

    // Write gate
    pub gate_passed: Counter,
    pub gate_rejected: Family<GateCheckLabel, Counter>,
    pub gate_skipped: Counter,

    // Search
    pub search_requests: Family<EndpointLabel, Counter>,
    pub search_duration: Family<EndpointLabel, Histogram>,

    // HTTP request counter (method × status)
    pub http_requests: Family<HttpLabel, Counter>,

    // Echo / fizzle
    pub echo_total_accesses: Gauge,
    pub echo_active_nodes: Gauge,

    // Uptime (set on each scrape)
    pub uptime_seconds: Gauge,
}

impl CortexMetrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        // Graph
        let nodes_by_kind: Family<KindLabel, Gauge> = Family::default();
        registry.register(
            "cortex_nodes_total",
            "Current number of nodes in the graph by kind",
            nodes_by_kind.clone(),
        );

        let edges_by_relation: Family<RelationLabel, Gauge> = Family::default();
        registry.register(
            "cortex_edges_total",
            "Current number of edges in the graph by relation",
            edges_by_relation.clone(),
        );

        let node_count: Gauge = Gauge::default();
        registry.register("cortex_node_count", "Total number of nodes", node_count.clone());

        let edge_count: Gauge = Gauge::default();
        registry.register("cortex_edge_count", "Total number of edges", edge_count.clone());

        let db_size: Gauge = Gauge::default();
        registry.register("cortex_db_size_bytes", "Database file size in bytes", db_size.clone());

        // Linker counters
        let linker_cycles: Counter = Counter::default();
        registry.register(
            "cortex_linker_cycles_total",
            "Total auto-linker cycles completed",
            linker_cycles.clone(),
        );

        let linker_edges_created: Counter = Counter::default();
        registry.register(
            "cortex_linker_edges_created_total",
            "Total edges created by the auto-linker",
            linker_edges_created.clone(),
        );

        let linker_edges_pruned: Counter = Counter::default();
        registry.register(
            "cortex_linker_edges_pruned_total",
            "Total edges pruned by decay",
            linker_edges_pruned.clone(),
        );

        let linker_edges_deleted: Counter = Counter::default();
        registry.register(
            "cortex_linker_edges_deleted_total",
            "Total edges deleted by decay",
            linker_edges_deleted.clone(),
        );

        let linker_duplicates_found: Counter = Counter::default();
        registry.register(
            "cortex_linker_duplicates_found_total",
            "Total duplicate nodes detected by the auto-linker",
            linker_duplicates_found.clone(),
        );

        let linker_contradictions_found: Counter = Counter::default();
        registry.register(
            "cortex_linker_contradictions_found_total",
            "Total contradictions detected by the auto-linker",
            linker_contradictions_found.clone(),
        );

        // Linker gauges
        let linker_backlog: Gauge = Gauge::default();
        registry.register(
            "cortex_linker_backlog",
            "Number of nodes awaiting auto-linking",
            linker_backlog.clone(),
        );

        let linker_last_cycle_nodes: Gauge = Gauge::default();
        registry.register(
            "cortex_linker_last_cycle_nodes_processed",
            "Nodes processed in the last auto-linker cycle",
            linker_last_cycle_nodes.clone(),
        );

        let linker_last_cycle_edges: Gauge = Gauge::default();
        registry.register(
            "cortex_linker_last_cycle_edges_created",
            "Edges created in the last auto-linker cycle",
            linker_last_cycle_edges.clone(),
        );

        let linker_cycle_duration =
            Histogram::new([0.05_f64, 0.1, 0.5, 1.0, 5.0].into_iter());
        registry.register(
            "cortex_linker_cycle_duration_seconds",
            "Auto-linker cycle duration in seconds",
            linker_cycle_duration.clone(),
        );

        // Write gate
        let gate_passed: Counter = Counter::default();
        registry.register(
            "cortex_gate_passed_total",
            "Write gate: nodes that passed all checks",
            gate_passed.clone(),
        );

        let gate_rejected: Family<GateCheckLabel, Counter> = Family::default();
        registry.register(
            "cortex_gate_rejected_total",
            "Write gate: nodes rejected, by failing check",
            gate_rejected.clone(),
        );

        let gate_skipped: Counter = Counter::default();
        registry.register(
            "cortex_gate_skipped_total",
            "Write gate: nodes that bypassed gate checks",
            gate_skipped.clone(),
        );

        // Search
        let search_requests: Family<EndpointLabel, Counter> = Family::default();
        registry.register(
            "cortex_search_requests_total",
            "Total search requests by endpoint type",
            search_requests.clone(),
        );

        let search_duration: Family<EndpointLabel, Histogram> =
            Family::new_with_constructor(|| {
                Histogram::new([0.01_f64, 0.05, 0.1, 0.5, 1.0].into_iter())
            });
        registry.register(
            "cortex_search_duration_seconds",
            "Search request duration in seconds",
            search_duration.clone(),
        );

        // HTTP
        let http_requests: Family<HttpLabel, Counter> = Family::default();
        registry.register(
            "cortex_http_requests_total",
            "Total HTTP requests by method and status code",
            http_requests.clone(),
        );

        // Echo
        let echo_total_accesses: Gauge = Gauge::default();
        registry.register(
            "cortex_echo_total_accesses",
            "Sum of all node access counts",
            echo_total_accesses.clone(),
        );

        let echo_active_nodes: Gauge = Gauge::default();
        registry.register(
            "cortex_echo_active_nodes",
            "Number of nodes with at least one access",
            echo_active_nodes.clone(),
        );

        // Uptime
        let uptime_seconds: Gauge = Gauge::default();
        registry.register(
            "cortex_uptime_seconds",
            "Server uptime in seconds",
            uptime_seconds.clone(),
        );

        Self {
            registry,
            nodes_by_kind,
            edges_by_relation,
            node_count,
            edge_count,
            db_size,
            linker_cycles,
            linker_edges_created,
            linker_edges_pruned,
            linker_edges_deleted,
            linker_duplicates_found,
            linker_contradictions_found,
            linker_backlog,
            linker_last_cycle_nodes,
            linker_last_cycle_edges,
            linker_cycle_duration,
            gate_passed,
            gate_rejected,
            gate_skipped,
            search_requests,
            search_duration,
            http_requests,
            echo_total_accesses,
            echo_active_nodes,
            uptime_seconds,
        }
    }
}
