"""
RAG pipeline using Cortex for hybrid retrieval.

Usage:
    cortex serve &
    python pipeline.py index ./docs/     # Index documents
    python pipeline.py query "question"  # Query
"""

import sys
import os
from pathlib import Path
from openai import OpenAI
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
llm = OpenAI()


def index_directory(directory: str):
    """Index all .md and .txt files in a directory as Cortex nodes."""
    path = Path(directory)
    files = list(path.rglob("*.md")) + list(path.rglob("*.txt"))
    print(f"Indexing {len(files)} files from {directory}...")

    for file in files:
        try:
            content = file.read_text(encoding="utf-8")
            # Simple chunking: split on double newlines
            chunks = [c.strip() for c in content.split("\n\n") if len(c.strip()) > 50]
            for i, chunk in enumerate(chunks):
                title = f"{file.name}:{i+1}" if i > 0 else file.name
                cx.store(
                    kind="fact",
                    title=title,
                    body=chunk,
                    source_agent="rag-indexer",
                    importance=0.6,
                    tags=["document", file.suffix.lstrip(".")],
                )
            print(f"  {file.name}: {len(chunks)} chunks")
        except Exception as e:
            print(f"  Error indexing {file}: {e}")

    print(f"\nIndexed. Run 'cortex node link --trigger' to build relationships.")


def query(question: str):
    """Retrieve relevant context and answer a question."""
    # Hybrid search for best retrieval
    results = cx.search(question, limit=5, hybrid=True)

    if not results:
        print("No relevant documents found.")
        return

    context = "\n\n".join(
        f"[{r.score:.2f}] {r.title}\n{r.body}"
        for r in results
    )

    response = llm.chat.completions.create(
        model="gpt-4o",
        messages=[
            {
                "role": "system",
                "content": (
                    "Answer the question based on the provided context. "
                    "Be concise and cite which documents informed your answer."
                ),
            },
            {
                "role": "user",
                "content": f"Context:\n{context}\n\nQuestion: {question}",
            },
        ],
    )
    print(response.choices[0].message.content)


def main():
    if len(sys.argv) < 2:
        print("Usage: python pipeline.py index <dir> | query <question>")
        sys.exit(1)

    command = sys.argv[1]
    if command == "index" and len(sys.argv) >= 3:
        index_directory(sys.argv[2])
    elif command == "query" and len(sys.argv) >= 3:
        query(" ".join(sys.argv[2:]))
    else:
        print("Usage: python pipeline.py index <dir> | query <question>")


if __name__ == "__main__":
    main()
