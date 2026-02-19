/**
 * Cortex client for TypeScript/Node.js.
 *
 * Connects to a running Cortex server over gRPC. The proto definition is loaded
 * at runtime via @grpc/proto-loader — no code generation step required.
 *
 * @example
 * ```typescript
 * import { Cortex } from '@cortex-memory/client';
 *
 * const cx = new Cortex('localhost:9090');
 *
 * const id = await cx.store({ kind: 'fact', title: 'API rate limit is 1000/min' });
 * const results = await cx.search('rate limits');
 * const briefing = await cx.briefing('my-agent');
 * console.log(briefing);
 * ```
 */

import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import path from 'path';

// Proto file is bundled inside the package at ../proto/cortex.proto
const PROTO_PATH = path.join(__dirname, '..', 'proto', 'cortex.proto');

const LOADER_OPTIONS: protoLoader.Options = {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
  includeDirs: [path.join(__dirname, '..', 'proto')],
};

/** Options for storing a node. */
export interface StoreOptions {
  kind: string;
  title: string;
  body?: string;
  tags?: string[];
  importance?: number;
  /** Metadata key-value pairs (string values). */
  metadata?: Record<string, string>;
  source_agent?: string;
}

/** A single similarity search result. */
export interface SearchResult {
  score: number;
  nodeId: string;
  title: string;
  kind: string;
  body: string;
  importance: number;
}

/** A subgraph returned by traversal. */
export interface Subgraph {
  nodes: unknown[];
  edges: unknown[];
  truncated: boolean;
}

/** A Cortex client connected to a gRPC server. */
export class Cortex {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private readonly client: any;

  /**
   * Create a client connected to *addr* (e.g. `"localhost:9090"`).
   */
  constructor(addr: string) {
    const packageDefinition = protoLoader.loadSync(PROTO_PATH, LOADER_OPTIONS);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const proto = grpc.loadPackageDefinition(packageDefinition) as any;
    this.client = new proto.cortex.v1.CortexService(
      addr,
      grpc.credentials.createInsecure(),
    );
  }

  // ------------------------------------------------------------------
  // Write
  // ------------------------------------------------------------------

  /** Store a knowledge node. Returns the new node ID. */
  async store(options: StoreOptions): Promise<string> {
    return this._call('CreateNode', {
      kind: options.kind,
      title: options.title,
      body: options.body ?? options.title,
      importance: options.importance ?? 0.5,
      tags: options.tags ?? [],
      metadata: options.metadata ?? {},
      source_agent: options.source_agent ?? '',
    }).then((r: { id: string }) => r.id);
  }

  // ------------------------------------------------------------------
  // Read / search
  // ------------------------------------------------------------------

  /**
   * Semantic similarity search. Returns ranked results.
   *
   * @param query  Natural-language query string.
   * @param opts   Optional `limit` (default 10) and `kindFilter`.
   */
  async search(
    query: string,
    opts: { limit?: number; kindFilter?: string[] } = {},
  ): Promise<SearchResult[]> {
    const resp = await this._call('SimilaritySearch', {
      query,
      limit: opts.limit ?? 10,
      kind_filter: opts.kindFilter ?? [],
    });
    // SearchResponse.results is SearchResultEntry[] where each has node + score
    return (resp.results ?? []).map((r: {
      score: number;
      node: { id: string; title: string; kind: string; body: string; importance: number };
    }) => ({
      score: r.score,
      nodeId: r.node?.id ?? '',
      title: r.node?.title ?? '',
      kind: r.node?.kind ?? '',
      body: r.node?.body ?? '',
      importance: r.node?.importance ?? 0,
    }));
  }

  /**
   * Hybrid search combining vector similarity with graph proximity.
   *
   * @param query      Natural-language query string.
   * @param anchorIds  Node IDs to anchor the graph proximity component.
   * @param opts       Optional `limit` (default 10).
   */
  async searchHybrid(
    query: string,
    anchorIds: string[] = [],
    opts: { limit?: number } = {},
  ): Promise<SearchResult[]> {
    const resp = await this._call('HybridSearch', {
      query,
      anchor_ids: anchorIds,
      limit: opts.limit ?? 10,
    });
    return (resp.results ?? []).map((r: {
      combined_score: number;
      node: { id: string; title: string; kind: string; body: string; importance: number };
    }) => ({
      score: r.combined_score,
      nodeId: r.node?.id ?? '',
      title: r.node?.title ?? '',
      kind: r.node?.kind ?? '',
      body: r.node?.body ?? '',
      importance: r.node?.importance ?? 0,
    }));
  }

  /**
   * Generate a context briefing for an agent.
   *
   * @param agentId  Agent identifier, e.g. `"kai"`.
   * @param compact  Use compact (~4× denser) rendering.
   * @returns Rendered markdown string.
   */
  async briefing(agentId: string, compact = false): Promise<string> {
    const resp = await this._call('GetBriefing', {
      agent_id: agentId,
      compact,
    });
    return resp.rendered ?? '';
  }

  /**
   * Graph traversal from *nodeId* up to *depth* hops.
   */
  async traverse(nodeId: string, depth = 2): Promise<Subgraph> {
    const resp = await this._call('Traverse', {
      start_ids: [nodeId],
      max_depth: depth,
    });
    return {
      nodes: resp.nodes ?? [],
      edges: resp.edges ?? [],
      truncated: resp.truncated ?? false,
    };
  }

  /** Get a node by ID. Returns `null` if not found. */
  async getNode(id: string): Promise<unknown | null> {
    try {
      return await this._call('GetNode', { id });
    } catch (err: unknown) {
      if (_isGrpcError(err, grpc.status.NOT_FOUND)) return null;
      throw err;
    }
  }

  // ------------------------------------------------------------------
  // Internal
  // ------------------------------------------------------------------

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private _call(method: string, req: unknown): Promise<any> {
    return new Promise((resolve, reject) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this.client[method] as any)(req, (err: Error | null, response: unknown) => {
        if (err) reject(err);
        else resolve(response);
      });
    });
  }
}

function _isGrpcError(err: unknown, code: number): boolean {
  return (
    typeof err === 'object' &&
    err !== null &&
    'code' in err &&
    (err as { code: number }).code === code
  );
}
