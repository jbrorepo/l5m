#!/usr/bin/env bash
# L5M 5-minute demo: build -> durable server -> remember -> recall ->
# tenant-isolation proof -> time-travel -> metering -> tamper-evident audit.
#
#   ./scripts/demo.sh            # uses port 18080, cleans up after itself
#
# Everything here is the real product over real HTTP — no mocks.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${L5M_DEMO_PORT:-18080}"
BASE="http://127.0.0.1:$PORT"
DATA_DIR="$(mktemp -d)"
WRITE_KEY="demo-write-$RANDOM"
ADMIN_KEY="demo-admin-$RANDOM"

say()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
note() { printf '\033[0;32m%s\033[0m\n' "$*"; }

cleanup() {
  [ -n "${SERVER_PID:-}" ] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$DATA_DIR" 2>/dev/null || true
}
trap cleanup EXIT

say "1/7 Building l5m-server (release)"
cargo build --release -p l5m-server 2>&1 | tail -1

say "2/7 Starting a durable, rate-limited server with scoped keys + audit"
L5M_BIND="127.0.0.1:$PORT" \
L5M_DATA_DIR="$DATA_DIR" \
L5M_AUDIT_LOG="$DATA_DIR/audit.jsonl" \
L5M_API_KEYS="$WRITE_KEY:write,$ADMIN_KEY:admin" \
L5M_RATE_PER_SEC=50 \
./target/release/l5m-server &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -sf "$BASE/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -sf "$BASE/healthz" >/dev/null || { echo "server failed to start"; exit 1; }
note "up at $BASE (WAL-durable: acknowledged writes survive restarts)"

req() { # req TENANT METHOD PATH [JSON]
  local tenant="$1" method="$2" path="$3" body="${4:-}"
  curl -s -X "$method" "$BASE$path" \
    -H "x-l5m-api-key: $WRITE_KEY" -H "x-l5m-tenant: $tenant" \
    -H 'content-type: application/json' ${body:+-d "$body"}
}

say "3/7 Tenant 7 remembers facts (one with a validity window)"
req 7 POST /v1/memories '{"capsule_id":"1","tenant_id":7,"claim":"the production db password is hunter2-kelpstone","evidence":"set during onboarding","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":9,"classification":1,"poison_risk":0}' >/dev/null
req 7 POST /v1/memories '{"capsule_id":"2","tenant_id":7,"claim":"the office was at 12 Amber Street","evidence":"lease v1","source_id":1,"valid_from":1000,"valid_until":5000,"observed_at":1000,"last_verified_at":1000,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}' >/dev/null
req 7 POST /v1/memories '{"capsule_id":"3","tenant_id":7,"claim":"the office is at 3 Cobalt Avenue","evidence":"lease v2","source_id":1,"valid_from":5000,"observed_at":5000,"last_verified_at":5000,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}' >/dev/null
note "3 memories stored"

say "4/7 Recall works for tenant 7 — and the SAME query leaks NOTHING to tenant 42"
T7=$(req 7 POST /v1/query '{"query":"production db password"}')
echo "$T7" | grep -q "hunter2-kelpstone" && note "tenant 7 recalls its secret ✓"
T42=$(req 42 POST /v1/query '{"query":"the production db password is hunter2-kelpstone"}')
if echo "$T42" | grep -q "hunter2"; then echo "LEAK — this must never happen"; exit 1; fi
note "tenant 42 (perfect-match query!) gets zero results — gates run BEFORE scoring ✓"

say "5/7 Time-travel: where was the office at t=2000 vs now?"
THEN=$(req 7 POST /v1/query '{"query":"where is the office","as_of":2000}')
NOW=$(req 7 POST /v1/query '{"query":"where is the office","as_of":9000}')
echo "$THEN" | grep -q "Amber Street"  && note "as_of=2000 -> 12 Amber Street ✓"
echo "$NOW"  | grep -q "Cobalt Avenue" && note "as_of=9000 -> 3 Cobalt Avenue ✓"

say "6/7 Metering (admin key): per-tenant usage"
curl -s "$BASE/v1/usage" -H "x-l5m-api-key: $ADMIN_KEY" -H "x-l5m-tenant: 0" | python -m json.tool 2>/dev/null || \
curl -s "$BASE/v1/usage" -H "x-l5m-api-key: $ADMIN_KEY" -H "x-l5m-tenant: 0"

say "7/7 Tamper-evident audit chain"
curl -s "$BASE/v1/audit/verify" -H "x-l5m-api-key: $WRITE_KEY" -H "x-l5m-tenant: 7"
echo
note "every disclosure above is in a hash-chained audit log; any edit breaks the chain"

say "Done"
echo "That was: durable writes, gate-before-scoring isolation, point-in-time"
echo "recall, per-tenant metering, and a verifiable audit trail — over real HTTP."
