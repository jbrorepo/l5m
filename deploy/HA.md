# Deployment topology & the path to HA

This document describes what L5M's deployment model is **today**, what its
architecture makes straightforward **next**, and the operational guardrails in
between. It is written to be handed to an SRE during evaluation — no
aspirational claims without a label.

## Today: single writer, durable by construction

One `l5m-server` process owns the data directory:

```
            ┌────────────────────────────┐
  clients → │  l5m-server (single node)  │
            │  /data/l5m.wal       (WAL) │
            │  /data/checkpoints/*.segment
            │  /data/audit.jsonl         │
            └────────────────────────────┘
```

- **Crash safety** is already strong: every acknowledged write is fsync'd to
  the WAL first; restart replays it (O(N)); admin checkpoints fold state into
  an immutable base segment and truncate the WAL in a crash-safe order; the
  newest checkpoint is auto-loaded on boot.
- **Recovery point objective (RPO): zero** for acknowledged writes on a
  surviving disk. **Recovery time objective (RTO):** process restart + WAL
  replay (keep the WAL short by checkpointing on a schedule — one
  `POST /v1/admin/checkpoint` cron).
- **The one hard rule:** never run two writers against one data directory.
  The Helm chart enforces `strategy: Recreate` and a single replica for this
  reason; Kubernetes will not start the new pod before the old one releases
  the PVC.

This is the same operational shape as single-node PostgreSQL or SQLite-backed
services: a perfectly normal place for a system of this size to be, and
honest to say so.

## Backup & restore (works today)

1. `POST /v1/admin/checkpoint` (admin scope) — produces a self-contained,
   self-authenticating segment file under `/data/checkpoints/`.
2. Copy that file (and `/data/audit.jsonl` if you want the audit trail)
   anywhere — object storage, another region. Segments are immutable, so the
   copy is consistent by construction; BLAKE3 self-hashes detect corruption
   on open.
3. Restore = place the segment in the new node's checkpoint dir and start the
   server. It loads the newest checkpoint automatically.

## Next: read replicas via segment shipping (designed, not yet built)

Immutable segments make the classic replication pattern unusually clean —
there is no page-level state to reconcile, only whole files that are either
present or not:

```
                       writes
  clients ────────────────────────▶ writer (WAL + checkpoints)
     │                                   │  ships sealed segments +
     │ reads (gated, any replica)        │  WAL tail to object storage
     ▼                                   ▼
  replica A ◀──── pulls segments ──── s3://bucket/segments/
  replica B ◀──── pulls segments ──────────┘
```

- Replicas serve `/v1/query` only (the scope model already distinguishes
  read from write credentials, so routing read keys at replicas is natural).
- Replication lag = checkpoint/shipping cadence; gates are evaluated *on the
  replica* from the segment contents, so **security does not depend on the
  freshness or the network path** — a stale replica returns stale-but-
  authorized data, never unauthorized data.
- Failover = promote a replica by giving it the data directory (or the
  newest shipped segment set + WAL tail) and the write credentials.

This is the labeled roadmap item; until it ships, scale reads by raising
single-node throughput (queries are sub-millisecond at 1M capsules) and scale
tenants horizontally by **sharding tenants across independent L5M instances**
— tenancy is enforced inside each engine, so a tenant→instance routing layer
needs no security logic of its own.

## Kubernetes quick start

```bash
# Scoped keys (dev) — create the secret, then install:
kubectl create secret generic l5m-keys \
  --from-literal=api-keys="$(openssl rand -hex 24):write,$(openssl rand -hex 24):admin"
helm install mem ./deploy/helm/l5m --set auth.apiKeysSecretName=l5m-keys

# Production auth — mount your IdP's JWKS instead:
kubectl create secret generic l5m-jwks --from-file=jwks.json
helm install mem ./deploy/helm/l5m --set auth.jwksSecretName=l5m-jwks
```

Pods run non-root with a read-only root filesystem and all capabilities
dropped; the only writable mounts are `/data` (PVC) and `/tmp` (emptyDir).

## docker-compose quick start

```bash
cd deploy
L5M_WRITE_KEY=$(openssl rand -hex 24) L5M_ADMIN_KEY=$(openssl rand -hex 24) \
  docker compose up -d
```
