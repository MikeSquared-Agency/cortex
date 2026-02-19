/**
 * Unit tests for the Cortex TypeScript SDK.
 * All tests use MockCortex — no real server or gRPC connection required.
 */
import { describe, it, expect, beforeEach } from '@jest/globals';
import { MockCortex } from '../testing';
describe('MockCortex', () => {
  let cx: MockCortex;

  beforeEach(() => {
    cx = new MockCortex();
  });

  // -----------------------------------------------------------------------
  // Constructor
  // -----------------------------------------------------------------------
  describe('constructor', () => {
    it('creates an empty instance with zero nodes', () => {
      expect(cx.nodeCount).toBe(0);
    });

    it('can create multiple independent instances', () => {
      const cx2 = new MockCortex();
      expect(cx).not.toBe(cx2);
      expect(cx.nodeCount).toBe(0);
      expect(cx2.nodeCount).toBe(0);
    });
  });

  // -----------------------------------------------------------------------
  // store()
  // -----------------------------------------------------------------------
  describe('store()', () => {
    it('returns a non-empty string ID', async () => {
      const id = await cx.store({ kind: 'fact', title: 'Test fact' });
      expect(typeof id).toBe('string');
      expect(id.length).toBeGreaterThan(0);
    });

    it('increments nodeCount on each call', async () => {
      await cx.store({ kind: 'fact', title: 'First' });
      expect(cx.nodeCount).toBe(1);
      await cx.store({ kind: 'event', title: 'Second' });
      expect(cx.nodeCount).toBe(2);
    });

    it('returns unique IDs for each call', async () => {
      const id1 = await cx.store({ kind: 'fact', title: 'Node A' });
      const id2 = await cx.store({ kind: 'fact', title: 'Node B' });
      expect(id1).not.toBe(id2);
    });

    it('accepts all optional fields without error', async () => {
      const id = await cx.store({
        kind: 'note',
        title: 'Annotated note',
        body: 'Extended body text',
        tags: ['alpha', 'beta'],
        importance: 0.8,
        metadata: { project: 'cortex', env: 'test' },
        source_agent: 'kai',
      });
      expect(typeof id).toBe('string');
    });

    it('records the call in the call log (assertStored passes)', async () => {
      await cx.store({ kind: 'fact', title: 'Logged fact' });
      expect(() => cx.assertStored('fact', 'Logged fact')).not.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // search()
  // -----------------------------------------------------------------------
  describe('search()', () => {
    it('returns results that match the query string', async () => {
      await cx.store({ kind: 'fact', title: 'Rate limit is 1000/min' });
      await cx.store({ kind: 'event', title: 'Deploy complete' });

      const results = await cx.search('rate limit');
      expect(results).toHaveLength(1);
      expect(results[0].title).toBe('Rate limit is 1000/min');
      expect(results[0].kind).toBe('fact');
      expect(results[0].score).toBe(0.9);
    });

    it('returns an empty array when nothing matches', async () => {
      await cx.store({ kind: 'fact', title: 'Totally unrelated' });
      const results = await cx.search('zyx-no-match-xyz');
      expect(results).toHaveLength(0);
    });

    it('matches on body text as well as title', async () => {
      await cx.store({ kind: 'note', title: 'Short title', body: 'detailed body content here' });
      const results = await cx.search('detailed body');
      expect(results).toHaveLength(1);
    });

    it('is case-insensitive', async () => {
      await cx.store({ kind: 'fact', title: 'Important Discovery' });
      const results = await cx.search('important discovery');
      expect(results).toHaveLength(1);
    });

    it('respects the limit option', async () => {
      for (let i = 0; i < 6; i++) {
        await cx.store({ kind: 'fact', title: `Item number ${i}` });
      }
      const results = await cx.search('Item', { limit: 3 });
      expect(results.length).toBeLessThanOrEqual(3);
    });

    it('returns all results when limit is higher than match count', async () => {
      await cx.store({ kind: 'fact', title: 'Just one match' });
      const results = await cx.search('one match', { limit: 100 });
      expect(results).toHaveLength(1);
    });
  });

  // -----------------------------------------------------------------------
  // briefing()
  // -----------------------------------------------------------------------
  describe('briefing()', () => {
    it('returns a non-empty string', async () => {
      const text = await cx.briefing('kai');
      expect(typeof text).toBe('string');
      expect(text.length).toBeGreaterThan(0);
    });

    it('includes the agent ID in the response', async () => {
      const text = await cx.briefing('my-agent');
      expect(text).toContain('my-agent');
    });

    it('accepts a compact flag without error', async () => {
      const text = await cx.briefing('kai', true);
      expect(typeof text).toBe('string');
    });
  });

  // -----------------------------------------------------------------------
  // getNode()
  // -----------------------------------------------------------------------
  describe('getNode()', () => {
    it('returns the node when found', async () => {
      const id = await cx.store({ kind: 'fact', title: 'Findable node', importance: 0.7 });
      const node = await cx.getNode(id);

      expect(node).not.toBeNull();
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const n = node as any;
      expect(n.id).toBe(id);
      expect(n.kind).toBe('fact');
      expect(n.title).toBe('Findable node');
      expect(n.importance).toBe(0.7);
    });

    it('returns null when the node ID does not exist', async () => {
      const node = await cx.getNode('nonexistent-node-id-99999');
      expect(node).toBeNull();
    });

    it('returns null for an empty string ID', async () => {
      const node = await cx.getNode('');
      expect(node).toBeNull();
    });
  });

  // -----------------------------------------------------------------------
  // traverse()
  // -----------------------------------------------------------------------
  describe('traverse()', () => {
    it('returns an object with nodes, edges, and truncated fields', async () => {
      const id = await cx.store({ kind: 'concept', title: 'Root node' });
      const subgraph = await cx.traverse(id, 2);

      expect(subgraph).toHaveProperty('nodes');
      expect(subgraph).toHaveProperty('edges');
      expect(subgraph).toHaveProperty('truncated');
    });

    it('nodes and edges are arrays', async () => {
      const id = await cx.store({ kind: 'concept', title: 'Graph start' });
      const subgraph = await cx.traverse(id);

      expect(Array.isArray(subgraph.nodes)).toBe(true);
      expect(Array.isArray(subgraph.edges)).toBe(true);
    });

    it('truncated is a boolean', async () => {
      const id = await cx.store({ kind: 'concept', title: 'Any node' });
      const subgraph = await cx.traverse(id, 1);

      expect(typeof subgraph.truncated).toBe('boolean');
    });

    it('uses default depth when not specified', async () => {
      const id = await cx.store({ kind: 'concept', title: 'Default depth' });
      // Should not throw
      const subgraph = await cx.traverse(id);
      expect(subgraph).toBeDefined();
    });
  });

  // -----------------------------------------------------------------------
  // assertStored() / assertNotStored() helpers
  // -----------------------------------------------------------------------
  describe('assertStored()', () => {
    it('passes when the call was made', async () => {
      await cx.store({ kind: 'fact', title: 'Known fact' });
      expect(() => cx.assertStored('fact', 'Known fact')).not.toThrow();
    });

    it('throws when the call was NOT made', () => {
      expect(() => cx.assertStored('fact', 'Missing fact')).toThrow();
    });

    it('is strict about kind', async () => {
      await cx.store({ kind: 'event', title: 'Same title' });
      expect(() => cx.assertStored('fact', 'Same title')).toThrow();
    });
  });

  describe('assertNotStored()', () => {
    it('passes when the call was NOT made', () => {
      expect(() => cx.assertNotStored('fact', 'Never stored')).not.toThrow();
    });

    it('throws when the call WAS made', async () => {
      await cx.store({ kind: 'fact', title: 'Stored fact' });
      expect(() => cx.assertNotStored('fact', 'Stored fact')).toThrow();
    });
  });
});
