/**
 * Testing utilities for the Cortex TypeScript SDK.
 *
 * ``MockCortex`` implements the same interface as ``Cortex`` but stores
 * everything in memory — no server or gRPC required.
 *
 * @example
 * ```typescript
 * import { MockCortex } from '@cortex-memory/client';
 *
 * describe('my agent', () => {
 *   it('stores and retrieves knowledge', async () => {
 *     const cx = new MockCortex();
 *     const id = await cx.store({ kind: 'fact', title: 'test fact' });
 *     const results = await cx.search('test');
 *     expect(results).toHaveLength(1);
 *     cx.assertStored('fact', 'test fact');
 *   });
 * });
 * ```
 */

import type { SearchResult, StoreOptions, Subgraph } from './client';

interface StoredNode {
  id: string;
  kind: string;
  title: string;
  body: string;
  tags: string[];
  importance: number;
  metadata: Record<string, string>;
  source_agent: string;
}

interface CallLogEntry {
  method: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  args: any[];
}

/** In-memory Cortex implementation for unit tests. */
export class MockCortex {
  private readonly nodes: Map<string, StoredNode> = new Map();
  private readonly callLog: CallLogEntry[] = [];

  // ------------------------------------------------------------------
  // Write
  // ------------------------------------------------------------------

  async store(options: StoreOptions): Promise<string> {
    const id = _uuid();
    const node: StoredNode = {
      id,
      kind: options.kind,
      title: options.title,
      body: options.body ?? options.title,
      tags: options.tags ?? [],
      importance: options.importance ?? 0.5,
      metadata: options.metadata ?? {},
      source_agent: options.source_agent ?? '',
    };
    this.nodes.set(id, node);
    this.callLog.push({ method: 'store', args: [options] });
    return id;
  }

  // ------------------------------------------------------------------
  // Read / search
  // ------------------------------------------------------------------

  async search(
    query: string,
    opts: { limit?: number; kindFilter?: string[] } = {},
  ): Promise<SearchResult[]> {
    const q = query.toLowerCase();
    const limit = opts.limit ?? 10;
    const results: SearchResult[] = [];
    for (const n of this.nodes.values()) {
      if (n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q)) {
        results.push({
          score: 0.9,
          nodeId: n.id,
          title: n.title,
          kind: n.kind,
          body: n.body,
          importance: n.importance,
        });
      }
      if (results.length >= limit) break;
    }
    return results;
  }

  async searchHybrid(
    query: string,
    _anchorIds: string[] = [],
    opts: { limit?: number } = {},
  ): Promise<SearchResult[]> {
    return this.search(query, opts);
  }

  async briefing(agentId: string, _compact = false): Promise<string> {
    return `[Mock briefing for ${agentId}]`;
  }

  async traverse(_nodeId: string, _depth = 2): Promise<Subgraph> {
    return { nodes: [], edges: [], truncated: false };
  }

  async getNode(id: string): Promise<StoredNode | null> {
    return this.nodes.get(id) ?? null;
  }

  // ------------------------------------------------------------------
  // Assertion helpers
  // ------------------------------------------------------------------

  /** Assert that ``store({ kind, title })`` was called. */
  assertStored(kind: string, title: string): void {
    const found = this.callLog.some(
      (e) =>
        e.method === 'store' &&
        e.args[0].kind === kind &&
        e.args[0].title === title,
    );
    if (!found) {
      throw new Error(
        `Expected store({ kind: ${JSON.stringify(kind)}, title: ${JSON.stringify(title)} }) ` +
          `but it was not called.\nCalls: ${JSON.stringify(this.callLog, null, 2)}`,
      );
    }
  }

  /** Assert that ``store({ kind, title })`` was NOT called. */
  assertNotStored(kind: string, title: string): void {
    const found = this.callLog.some(
      (e) =>
        e.method === 'store' &&
        e.args[0].kind === kind &&
        e.args[0].title === title,
    );
    if (found) {
      throw new Error(
        `Expected store({ kind: ${JSON.stringify(kind)}, title: ${JSON.stringify(title)} }) ` +
          `NOT to be called, but it was.`,
      );
    }
  }

  /** Number of nodes stored so far. */
  get nodeCount(): number {
    return this.nodes.size;
  }
}

function _uuid(): string {
  // Prefer native crypto.randomUUID if available (Node ≥ 14.17)
  if (
    typeof globalThis !== 'undefined' &&
    typeof (globalThis as unknown as { crypto?: { randomUUID?: () => string } }).crypto
      ?.randomUUID === 'function'
  ) {
    return (globalThis as unknown as { crypto: { randomUUID: () => string } }).crypto.randomUUID();
  }
  // Fallback
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
  });
}
