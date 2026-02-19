# OpenClaw / Warren Integration

Cortex is the native memory engine for OpenClaw agents running under the Warren swarm framework. This guide covers the built-in integration.

## Warren Adapter

The `warren-adapter` crate automatically maps Warren NATS events to Cortex nodes. It ships as an optional feature in `cortex-server`:

```toml
# This is the default — warren is enabled by default
[features]
default = ["warren"]
warren = ["warren-adapter"]
```

To build without Warren (standalone mode):

```bash
cargo build -p cortex-server --no-default-features
```

## NATS Configuration

OpenClaw agents publish events to NATS subjects like `warren.<agent_id>.event`. Configure Cortex to subscribe:

```toml
# cortex.toml
[ingest.nats]
url = "nats://localhost:4222"
subjects = ["warren.>"]
```

The Warren adapter maps each NATS message to a typed node based on the event payload.

## Briefing Integration

The `GetBriefing` gRPC call is called by the OpenClaw context injection system at the start of each agent turn. The agent ID comes from the Warren session context:

```python
# Inside OpenClaw agent framework
briefing = cortex_client.GetBriefing(agent_id=session.agent_id)
# → Injected as system context
```

## Shared Memory Across Agents

Multiple Warren agents can share a single Cortex instance. Each agent writes to its own namespace using `source_agent`, and briefings are scoped by `agent_id`. Cross-agent knowledge discovery happens automatically via the auto-linker.

## Full Example

See [`examples/openclaw-agent/`](../../examples/openclaw-agent/) for a complete Warren + Cortex setup.
