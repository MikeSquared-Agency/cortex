# LangChain Agent with Cortex Memory

A LangChain conversational agent that uses Cortex as its long-term memory backend.

## Setup

1. Start Cortex: `cortex serve`
2. Install deps: `pip install -r requirements.txt`
3. Set your OpenAI key: `export OPENAI_API_KEY=sk-...`
4. Run: `python agent.py`

## What It Does

- Loads a context briefing from Cortex at the start of each turn
- Stores each conversation turn as an event node
- Over time, the auto-linker builds connections between related conversations
- The agent's knowledge accumulates and persists across restarts
