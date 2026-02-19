"""
CrewAI multi-agent team with shared Cortex memory.

Usage:
    cortex serve &
    export OPENAI_API_KEY=sk-...
    python crew.py "Research topic here"
"""

import sys
from crewai import Agent, Task, Crew, Process
from crewai_tools import BaseTool
from cortex_memory import Cortex


class CortexMemoryTool(BaseTool):
    """Tool for storing and retrieving knowledge from the team's Cortex graph."""

    name: str = "memory"
    description: str = (
        "Store and retrieve knowledge from the shared team memory graph. "
        "Actions: 'store' (store a fact), 'search' (find related facts), "
        "'briefing' (get full team context)."
    )

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
            return f"Stored memory (id: {node_id})"
        elif action == "search":
            results = self.cx.search(query, limit=5)
            if not results:
                return "No relevant memories found."
            return "\n".join(f"- [{r.score:.2f}] {r.title}" for r in results)
        elif action == "briefing":
            briefing = self.cx.briefing("team")
            return briefing if briefing.strip() else "Memory graph is empty."
        return f"Unknown action '{action}'. Use: store, search, briefing"


def run_crew(topic: str):
    memory_tool = CortexMemoryTool()

    researcher = Agent(
        role="Research Specialist",
        goal=f"Thoroughly research '{topic}' and store all findings in team memory",
        backstory="You are an expert researcher who carefully stores all findings.",
        tools=[memory_tool],
        verbose=True,
    )

    writer = Agent(
        role="Technical Writer",
        goal="Retrieve research from team memory and write a comprehensive summary",
        backstory="You are a skilled writer who synthesises research into clear reports.",
        tools=[memory_tool],
        verbose=True,
    )

    research_task = Task(
        description=(
            f"Research '{topic}'. For each key finding, use the memory tool "
            f"with action='store' to save it. Store at least 5 distinct facts."
        ),
        agent=researcher,
        expected_output="Confirmation that findings have been stored in memory.",
    )

    write_task = Task(
        description=(
            "Use the memory tool with action='briefing' to retrieve all team knowledge. "
            "Then write a 300-word summary report based on the stored research."
        ),
        agent=writer,
        expected_output="A well-structured 300-word research summary.",
    )

    crew = Crew(
        agents=[researcher, writer],
        tasks=[research_task, write_task],
        process=Process.sequential,
        verbose=True,
    )

    result = crew.kickoff()
    print("\n=== FINAL REPORT ===")
    print(result)


if __name__ == "__main__":
    topic = sys.argv[1] if len(sys.argv) > 1 else "AI agent memory solutions"
    run_crew(topic)
