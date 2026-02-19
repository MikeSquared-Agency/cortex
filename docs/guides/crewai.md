# CrewAI Integration

Share memory across a CrewAI multi-agent team using Cortex.

## Install

```bash
pip install cortex-memory crewai crewai-tools
```

## Shared Memory Tool

```python
from cortex_memory import Cortex
from crewai_tools import BaseTool

class CortexMemoryTool(BaseTool):
    name: str = "memory"
    description: str = "Store and retrieve knowledge from the team memory graph"

    def __init__(self, cortex_addr: str = "localhost:9090"):
        super().__init__()
        self.cx = Cortex(cortex_addr)

    def _run(self, action: str, content: str = "", query: str = "") -> str:
        if action == "store":
            node_id = self.cx.store(
                kind="fact",
                title=content[:80],
                body=content,
                source_agent="team",
            )
            return f"Stored memory: {node_id}"
        elif action == "search":
            results = self.cx.search(query, limit=5)
            return "\n".join(f"{r.score:.2f}: {r.title}" for r in results)
        elif action == "briefing":
            return self.cx.briefing("team")
        return f"Unknown action: {action}"
```

## Usage

```python
from crewai import Agent, Task, Crew

memory_tool = CortexMemoryTool()

researcher = Agent(
    role="Researcher",
    goal="Research topics and store findings in shared memory",
    tools=[memory_tool],
)

writer = Agent(
    role="Writer",
    goal="Retrieve stored research and write comprehensive reports",
    tools=[memory_tool],
)

research_task = Task(
    description="Research the current state of AI memory solutions",
    agent=researcher,
)

write_task = Task(
    description="Write a report based on the research stored in memory",
    agent=writer,
)

crew = Crew(
    agents=[researcher, writer],
    tasks=[research_task, write_task],
)
crew.kickoff()
```

## How It Works

All agents in the crew share the same Cortex instance. The researcher stores findings as `fact` nodes; the writer retrieves them via `briefing("team")` which synthesises the most relevant recent knowledge.

The auto-linker runs in the background, automatically connecting related facts discovered by different agents.

## Full Example

See [`examples/crewai-team/`](../../examples/crewai-team/) in the repository.
