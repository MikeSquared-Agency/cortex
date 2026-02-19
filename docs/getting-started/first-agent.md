# Build Your First Agent with Cortex Memory

This guide builds a simple question-answering agent that remembers facts across conversations.

## Prerequisites

- Cortex server running at `localhost:9090` (see [Quick Start](./quickstart.md))
- Python 3.10+
- An OpenAI API key

## Install

```bash
pip install cortex-memory openai
```

## The Agent

```python
import os
from openai import OpenAI
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
llm = OpenAI()

AGENT_ID = "my-assistant"

def chat(user_message: str) -> str:
    # Get agent briefing — everything relevant the agent should know
    briefing = cx.briefing(AGENT_ID)

    response = llm.chat.completions.create(
        model="gpt-4o",
        messages=[
            {"role": "system", "content": f"You are a helpful assistant.\n\n{briefing}"},
            {"role": "user", "content": user_message},
        ]
    )
    reply = response.choices[0].message.content

    # Store the interaction as an event node
    cx.store(
        kind="event",
        title=user_message[:80],
        body=f"User: {user_message}\nAssistant: {reply}",
        source_agent=AGENT_ID,
        importance=0.5,
    )

    return reply

# Run a simple REPL
while True:
    msg = input("You: ")
    if msg.lower() in ("exit", "quit"):
        break
    print(f"Assistant: {chat(msg)}")
```

## What's Happening

1. At the start of each turn, `cx.briefing(AGENT_ID)` generates a tailored context document from the knowledge graph — recent events, patterns, goals, and any relevant facts.
2. After the turn, the conversation is stored as an `event` node. The auto-linker runs in the background and will wire related events together.
3. Over time, the graph builds up structured memory that the agent draws on automatically.

## Next Steps

- [LangChain integration](../guides/langchain.md) — drop-in memory for LangChain agents
- [CrewAI integration](../guides/crewai.md) — shared memory for multi-agent teams
- [Configuration](./configuration.md) — tune decay, retention, and briefing sections
