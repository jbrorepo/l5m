/**
 * L5M TypeScript client — a thin, dependency-free SDK for the L5M
 * security-gated memory server.
 *
 * L5M enforces security gates (tenant, context, policy, temporal, trust)
 * BEFORE relevance scoring, so an unauthorized memory is never even a
 * retrieval candidate. This client never sends the principal in the request
 * body — identity travels in auth headers or the bearer token, exactly as the
 * server expects.
 *
 * Zero runtime dependencies: built on the standard `fetch` (Node 18+,
 * browsers, edge runtimes).
 *
 * ```ts
 * import { Client } from "l5m-client";
 *
 * // API-key + header auth (dev / trusted network):
 * const c = new Client("http://localhost:8080", { apiKey: "secret", tenantId: 7 });
 * await c.insert({ capsule_id: "1", claim: "the launch is in March", ... });
 * const res = await c.query("when is the launch?");
 *
 * // JWT bearer auth (production): tenant/policy/trust come from verified
 * // claims, so you don't pass tenantId.
 * const prod = new Client("https://l5m.internal", { bearerToken: myJwt });
 * ```
 */

/** Base error. `status` is the HTTP status (undefined for transport failures). */
export class L5mError extends Error {
  readonly status?: number;
  constructor(message: string, status?: number) {
    super(message);
    this.name = "L5mError";
    this.status = status;
  }
}

/** 401/403 — missing or invalid credentials, or insufficient key scope. */
export class AuthError extends L5mError {
  constructor(message: string, status?: number) {
    super(message, status);
    this.name = "AuthError";
  }
}

/** 429 — per-tenant rate limit exceeded; back off and retry. */
export class RateLimited extends L5mError {
  constructor(message: string, status?: number) {
    super(message, status);
    this.name = "RateLimited";
  }
}

/** A memory capsule in the JSON shape the server ingests. */
export interface Capsule {
  capsule_id: string;
  /** Ignored on write — the server forces the authenticated tenant. */
  tenant_id?: number;
  claim: string;
  evidence: string;
  source_id: number;
  source_uri?: string;
  valid_from: number;
  valid_until?: number;
  observed_at: number;
  last_verified_at: number;
  /** Hex bitmask, e.g. "0xffff". */
  context_mask: string;
  /** Hex bitmask, e.g. "0xffff". */
  policy_mask: string;
  /** 0-10. */
  trust_level: number;
  classification: number;
  poison_risk: number;
  anchors?: string[];
  entities?: string[];
  /** Optional precomputed dense embedding. */
  embedding?: number[];
}

export interface QueryOptions {
  maxCapsules?: number;
  /** Point-in-time recall (unix seconds): only memories valid at that instant. */
  asOf?: number;
  /** Retrieval mode: "l5m" | "parent-aggregate" | "hybrid" | "rrf-parent". */
  mode?: string;
  /** Optional dense query embedding enabling hybrid lexical + dense ranking. */
  embedding?: number[];
}

export interface FrameCapsule {
  capsule_id: number | string;
  claim: string;
  evidence: string;
  trust_level: number;
  valid_from: number;
  valid_until: number | null;
  source_id: number;
  /** BLAKE3 source hash — proof-bearing provenance. */
  source_hash: number[];
  score: number;
  [extra: string]: unknown;
}

export interface QueryResponse {
  frame: {
    epoch: number;
    capsules: FrameCapsule[];
    conflicts: FrameCapsule[];
    coverage: Record<string, unknown>;
  };
  mode: string;
  segment_count: number;
  total_retrieval_ns: number;
  [extra: string]: unknown;
}

/** One row of per-tenant metering from GET /v1/usage (admin scope). */
export interface UsageRow {
  /** Tenant id, or "other" for the cardinality-overflow bucket. */
  tenant: number | "other";
  queries: number;
  capsules_returned: number;
  inserts: number;
  deletes: number;
}

export interface ClientOptions {
  /** API key for header auth (with `tenantId`). */
  apiKey?: string;
  /** JWT for bearer auth — the principal comes from verified claims. */
  bearerToken?: string;
  /** Tenant for header auth. Required unless `bearerToken` is set. */
  tenantId?: number;
  /** Hex context mask (default "0xffff"). */
  contextMask?: string;
  /** Hex policy mask (default "0xffff"). */
  policyMask?: string;
  /** Minimum trust recalled memories must meet (default 0). */
  trustFloor?: number;
  /** Request timeout in milliseconds (default 10_000). */
  timeoutMs?: number;
  /** Override fetch (tests / custom transports). */
  fetch?: typeof fetch;
}

export class Client {
  private readonly baseUrl: string;
  private readonly opts: ClientOptions;
  private readonly fetchImpl: typeof fetch;

  constructor(baseUrl: string, opts: ClientOptions) {
    if (!opts.bearerToken && opts.tenantId === undefined) {
      throw new Error("provide tenantId (header auth) or bearerToken (JWT auth)");
    }
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.opts = opts;
    this.fetchImpl = opts.fetch ?? fetch;
  }

  private headers(): Record<string, string> {
    const h: Record<string, string> = { "content-type": "application/json" };
    if (this.opts.bearerToken) {
      h["authorization"] = `Bearer ${this.opts.bearerToken}`;
    } else {
      if (this.opts.apiKey) h["x-l5m-api-key"] = this.opts.apiKey;
      h["x-l5m-tenant"] = String(this.opts.tenantId);
      h["x-l5m-context"] = this.opts.contextMask ?? "0xffff";
      h["x-l5m-policy"] = this.opts.policyMask ?? "0xffff";
      h["x-l5m-trust"] = String(this.opts.trustFloor ?? 0);
    }
    return h;
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.opts.timeoutMs ?? 10_000);
    let response: Response;
    try {
      response = await this.fetchImpl(`${this.baseUrl}${path}`, {
        method,
        headers: this.headers(),
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: controller.signal,
      });
    } catch (err) {
      throw new L5mError(`${method} ${path} failed: ${String(err)}`);
    } finally {
      clearTimeout(timer);
    }

    if (!response.ok) {
      const detail = await response.text().catch(() => "");
      const message = `${method} ${path} -> ${response.status}: ${detail}`;
      if (response.status === 401 || response.status === 403) {
        throw new AuthError(message, response.status);
      }
      if (response.status === 429) {
        throw new RateLimited(message, response.status);
      }
      throw new L5mError(message, response.status);
    }
    const contentType = response.headers.get("content-type") ?? "";
    if (contentType.includes("application/json")) {
      return (await response.json()) as T;
    }
    return (await response.text()) as unknown as T;
  }

  /** Insert/update a memory. The server forces tenant ownership regardless of
   * any `tenant_id` in the body. */
  insert(capsule: Capsule): Promise<{ status: string }> {
    return this.request("POST", "/v1/memories", capsule);
  }

  /** Insert several memories (one request per capsule — no batch endpoint yet). */
  async insertMany(capsules: Capsule[]): Promise<{ status: string }[]> {
    const out: { status: string }[] = [];
    for (const capsule of capsules) {
      out.push(await this.insert(capsule));
    }
    return out;
  }

  /** Run a gated retrieval. Returns the full server response (frame + coverage
   * + metadata). */
  query(text: string, options: QueryOptions = {}): Promise<QueryResponse> {
    const body: Record<string, unknown> = {
      query: text,
      max_capsules: options.maxCapsules ?? 8,
    };
    if (options.asOf !== undefined) body["as_of"] = options.asOf;
    if (options.mode !== undefined) body["mode"] = options.mode;
    if (options.embedding !== undefined) body["embedding"] = options.embedding;
    return this.request("POST", "/v1/query", body);
  }

  /** Hide a memory from all future results (delete / supersede). */
  delete(capsuleId: number | string): Promise<{ status: string }> {
    return this.request("DELETE", `/v1/memories/${capsuleId}`);
  }

  /** Per-tenant usage metering. Requires an admin-scope credential. */
  async usage(): Promise<UsageRow[]> {
    const res = await this.request<{ tenants: UsageRow[] }>("GET", "/v1/usage");
    return res.tenants;
  }

  /** Verify the tamper-evident audit chain. */
  verifyAudit(): Promise<{ intact: boolean; verified: number }> {
    return this.request("GET", "/v1/audit/verify");
  }

  /** Prometheus metrics exposition (text). */
  metrics(): Promise<string> {
    return this.request("GET", "/metrics");
  }

  /** True if the server is up. */
  async healthz(): Promise<boolean> {
    try {
      await this.request("GET", "/healthz");
      return true;
    } catch {
      return false;
    }
  }
}
