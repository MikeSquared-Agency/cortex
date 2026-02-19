# Migrating to Cortex

## From a Vector Database (Chroma, pgvector, Pinecone)

If you're moving from a pure vector DB, your existing embeddings and documents can be imported as Cortex nodes.

### Import from CSV

```bash
cortex import nodes data.csv --format csv
```

CSV format:
```
title,body,kind,importance,tags
"JWT auth fact","The API uses JWT","fact",0.7,"auth,api"
```

### Import from JSON

```bash
cortex import nodes data.json --format json
```

JSON format:
```json
[
  {
    "kind": "fact",
    "title": "The API uses JWT authentication",
    "body": "We chose JWT for stateless auth...",
    "importance": 0.7,
    "tags": ["auth", "api"]
  }
]
```

After import, run a manual auto-linker cycle to build the graph structure:

```bash
cortex node link --trigger
```

## From LangChain ConversationBufferMemory

LangChain's built-in memory stores conversations as flat text. To migrate:

1. Export your conversation history
2. Split into individual turns
3. Import each turn as an `event` node

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

for turn in conversation_history:
    cx.store(
        kind="event",
        title=turn["input"][:80],
        body=f"User: {turn['input']}\nAssistant: {turn['output']}",
        source_agent="migrated",
        importance=0.5,
    )
```

## From MEMORY.md / Flat Files

```bash
# Import a markdown file as chunked nodes
cortex import file memory.md --chunk-size 500

# Import a directory
cortex import dir ./docs/ --extensions md,txt
```

## Schema Migration

Cortex handles internal schema migrations automatically. If you encounter a schema version error:

```bash
cortex migrate
```
