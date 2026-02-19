# CrewAI Team with Shared Cortex Memory

A CrewAI research + writing crew that shares memory via Cortex.

## Setup

1. Start Cortex: `cortex serve`
2. Install deps: `pip install -r requirements.txt`
3. Set your OpenAI key: `export OPENAI_API_KEY=sk-...`
4. Run: `python crew.py`

## What It Does

- A Researcher agent stores findings as `fact` nodes in Cortex
- A Writer agent retrieves team knowledge via `briefing("team")`
- The auto-linker connects related findings across agents
- All knowledge persists between runs
