# RAG Pipeline with Cortex

Use Cortex as a RAG (Retrieval-Augmented Generation) backend with hybrid search.

## Setup

1. Start Cortex: `cortex serve`
2. Install deps: `pip install -r requirements.txt`
3. Index your documents: `python pipeline.py index ./docs/`
4. Query: `python pipeline.py query "your question here"`

## Features

- Index markdown/text files as Cortex nodes
- Hybrid search (vector + graph) for retrieval
- Automatic relationship discovery between documents
- Better than pure vector search for structured knowledge
