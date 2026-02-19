# TypeScript SDK Reference

## Install

```bash
npm install @cortex-memory/client
# or
yarn add @cortex-memory/client
```

## Quick Start

```typescript
import { Cortex } from '@cortex-memory/client';

const cx = new Cortex('localhost:9090');

await cx.store({
  kind: 'fact',
  title: 'The API uses JWT authentication',
  importance: 0.8,
});

const results = await cx.search('authentication', { limit: 5 });
const briefing = await cx.briefing('my-agent');
```

## new Cortex(addr)

Connect to a Cortex server.

- `addr` — gRPC address, e.g. `"localhost:9090"`

## cx.store(options) → Promise\<string\>

Store a new node. Returns the node ID.

```typescript
interface StoreOptions {
  kind: string;
  title: string;
  body?: string;
  importance?: number;     // 0.0–1.0, default 0.5
  tags?: string[];
  sourceAgent?: string;
  metadata?: Record<string, string>;
}

const id = await cx.store({
  kind: 'decision',
  title: 'Use TypeScript for the frontend',
  body: 'Strong typing reduces bugs in large codebases.',
  importance: 0.9,
  tags: ['architecture', 'frontend'],
});
```

## cx.search(query, options?) → Promise\<SearchResult[]\>

Search nodes semantically.

```typescript
interface SearchOptions {
  limit?: number;          // default 10
  kind?: string;           // filter by kind
}

interface SearchResult {
  id: string;
  title: string;
  body: string;
  kind: string;
  score: number;
  importance: number;
}

const results = await cx.search('authentication', { limit: 5, kind: 'fact' });
for (const r of results) {
  console.log(`${r.score.toFixed(2)} ${r.title}`);
}
```

## cx.briefing(agentId, options?) → Promise\<string\>

Get a context briefing for an agent.

```typescript
interface BriefingOptions {
  maxTokens?: number;      // default 2000
}

const context = await cx.briefing('my-agent');
// Inject into LLM system prompt
```

## cx.get(nodeId) → Promise\<Node\>

Get a node by ID.

## cx.delete(nodeId) → Promise\<void\>

Delete a node.

## cx.edge(fromId, toId, relation, options?) → Promise\<string\>

Create an edge between two nodes. Returns the edge ID.

```typescript
await cx.edge(nodeA, nodeB, 'supports', { weight: 0.9 });
```

## MockCortex

For testing — in-memory implementation of the Cortex interface:

```typescript
import { MockCortex } from '@cortex-memory/client/testing';

const cx = new MockCortex();
await cx.store({ kind: 'fact', title: 'Test fact' });
const results = await cx.search('test');
```

## Source

SDK lives in `sdks/typescript/` in the repository.
