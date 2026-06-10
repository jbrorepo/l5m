// Client tests against an in-process stub server (node:http, node:test —
// zero test dependencies). The stub asserts the client sends the right
// headers, method, path, and body, and that status codes map to the right
// error classes.

import assert from "node:assert/strict";
import http from "node:http";
import { test, before, after } from "node:test";

import { AuthError, Client, L5mError, RateLimited } from "../src/index.js";

interface Captured {
  method: string;
  url: string;
  headers: http.IncomingHttpHeaders;
  body: unknown;
}

let server: http.Server;
let baseUrl: string;
const last: { req?: Captured } = {};

before(async () => {
  server = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      last.req = {
        method: req.method ?? "",
        url: req.url ?? "",
        headers: req.headers,
        body: raw ? JSON.parse(raw) : undefined,
      };
      const send = (code: number, obj: unknown) => {
        const body = JSON.stringify(obj);
        res.writeHead(code, { "content-type": "application/json" });
        res.end(body);
      };

      // Simulated auth / rate-limit behavior for assertions.
      if (req.headers["x-l5m-api-key"] === "wrong") return send(401, { error: "unauthorized" });
      if (req.headers["x-l5m-api-key"] === "lowscope") return send(403, { error: "scope" });
      if (req.headers["x-l5m-tenant"] === "999") return send(429, { error: "rate limit exceeded" });

      if (req.url === "/healthz") {
        res.writeHead(200, { "content-type": "text/plain" });
        return res.end("ok");
      }
      if (req.url === "/metrics") {
        res.writeHead(200, { "content-type": "text/plain; version=0.0.4" });
        return res.end("l5m_queries_total 3\n");
      }
      if (req.url === "/v1/audit/verify") return send(200, { intact: true, verified: 2 });
      if (req.url === "/v1/usage")
        return send(200, {
          tenants: [{ tenant: 1, queries: 2, capsules_returned: 4, inserts: 1, deletes: 0 }],
        });
      if (req.url === "/v1/memories" && req.method === "POST") return send(201, { status: "inserted" });
      if (req.url?.startsWith("/v1/memories/") && req.method === "DELETE")
        return send(200, { status: "deleted" });
      if (req.url === "/v1/query")
        return send(200, { frame: { capsules: [{ score: 1.0, claim: "hi" }] } });
      return send(404, { error: "not found" });
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  if (addr === null || typeof addr === "string") throw new Error("no addr");
  baseUrl = `http://127.0.0.1:${addr.port}`;
});

after(() => server.close());

function client(extra: Partial<ConstructorParameters<typeof Client>[1]> = {}): Client {
  return new Client(baseUrl, { apiKey: "secret", tenantId: 7, ...extra });
}

test("requires tenantId or bearerToken", () => {
  assert.throws(() => new Client(baseUrl, { apiKey: "k" }), /tenantId .* bearerToken/);
});

test("insert sends headers and body", async () => {
  const out = await client().insert({
    capsule_id: "1",
    claim: "x",
    evidence: "x",
    source_id: 1,
    valid_from: 1,
    observed_at: 1,
    last_verified_at: 1,
    context_mask: "0xffff",
    policy_mask: "0xffff",
    trust_level: 8,
    classification: 1,
    poison_risk: 0,
  });
  assert.deepEqual(out, { status: "inserted" });
  assert.equal(last.req?.method, "POST");
  assert.equal(last.req?.url, "/v1/memories");
  assert.equal(last.req?.headers["x-l5m-api-key"], "secret");
  assert.equal(last.req?.headers["x-l5m-tenant"], "7");
  assert.equal((last.req?.body as { claim: string }).claim, "x");
});

test("query returns frame and forwards options", async () => {
  const res = await client().query("hello", { maxCapsules: 3, asOf: 2000 });
  assert.equal(res.frame.capsules[0]?.claim, "hi");
  assert.deepEqual(last.req?.body, { query: "hello", max_capsules: 3, as_of: 2000 });
});

test("bearer auth sets authorization and omits tenant headers", async () => {
  const c = new Client(baseUrl, { bearerToken: "tok.tok.tok" });
  await c.query("hi");
  assert.equal(last.req?.headers["authorization"], "Bearer tok.tok.tok");
  assert.equal(last.req?.headers["x-l5m-tenant"], undefined);
});

test("delete hits the id path", async () => {
  assert.deepEqual(await client().delete(42), { status: "deleted" });
  assert.equal(last.req?.method, "DELETE");
  assert.equal(last.req?.url, "/v1/memories/42");
});

test("usage returns metering rows", async () => {
  const rows = await client().usage();
  assert.equal(rows[0]?.tenant, 1);
  assert.equal(rows[0]?.queries, 2);
});

test("401 maps to AuthError", async () => {
  await assert.rejects(client({ apiKey: "wrong" }).query("x"), AuthError);
});

test("403 (insufficient scope) maps to AuthError", async () => {
  await assert.rejects(client({ apiKey: "lowscope" }).usage(), AuthError);
});

test("429 maps to RateLimited", async () => {
  await assert.rejects(client({ tenantId: 999 }).query("x"), RateLimited);
});

test("verifyAudit and metrics and healthz", async () => {
  assert.deepEqual(await client().verifyAudit(), { intact: true, verified: 2 });
  assert.match(await client().metrics(), /l5m_queries_total/);
  assert.equal(await client().healthz(), true);
});

test("unreachable server raises L5mError", async () => {
  const c = new Client("http://127.0.0.1:1", { apiKey: "k", tenantId: 1, timeoutMs: 1000 });
  await assert.rejects(c.query("x"), L5mError);
});
