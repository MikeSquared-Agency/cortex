# @cortex-memory/client

TypeScript/Node.js client SDK for the [Cortex](https://github.com/MikeSquared-Agency/cortex) graph memory engine.

## Installation

```bash
npm install @cortex-memory/client
```

## Quick start

```typescript
import { Cortex } from '@cortex-memory/client';

const cx = new Cortex('localhost:9090');

// Store knowledge
const id = await cx.store({
  kind: 'fact',
  title: 'API rate limit is 1000/min',
  tags: ['api', 'limits'],
});

// Semantic search
const results = await cx.search('rate limits');
for (const r of results) {
  console.log(`${r.score.toFixed(2)} — ${r.title}`);
}

// Get briefing
const briefing = await cx.briefing('my-agent');
console.log(briefing);
```

## Testing

```typescript
import { MockCortex } from '@cortex-memory/client';

describe('my agent', () => {
  it('stores knowledge', async () => {
    const cx = new MockCortex();
    await cx.store({ kind: 'fact', title: 'test fact' });
    const results = await cx.search('test');
    expect(results).toHaveLength(1);
    cx.assertStored('fact', 'test fact');
  });
});
```
