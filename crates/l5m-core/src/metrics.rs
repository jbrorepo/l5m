//! Lightweight, dependency-free metrics in Prometheus text-exposition format.
//!
//! Thread-safe via atomics (cheap on the hot path). The server (`server`
//! feature) exposes these at `/metrics`; libraries can scrape `render_prometheus`
//! directly. Kept in-house to honor the project's minimal-dependency value.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Cardinality guard: per-tenant series beyond this many distinct tenants are
/// aggregated into a single overflow row, so a tenant-id-spraying client
/// cannot blow up metrics memory or Prometheus cardinality.
const MAX_TRACKED_TENANTS: usize = 10_000;

/// The synthetic key usage beyond [`MAX_TRACKED_TENANTS`] is folded into.
const OVERFLOW_TENANT: u64 = u64::MAX;

/// Per-tenant usage counters — the metering record for billing/quotas as well
/// as per-customer observability.
#[derive(Clone, Copy, Debug, Default)]
pub struct TenantUsage {
    pub queries: u64,
    pub capsules_returned: u64,
    pub inserts: u64,
    pub deletes: u64,
}

/// Upper bounds (nanoseconds) for the latency histogram buckets.
const BUCKET_BOUNDS_NS: [u64; 13] = [
    100_000,       // 0.1 ms
    250_000,       // 0.25 ms
    500_000,       // 0.5 ms
    1_000_000,     // 1 ms
    2_500_000,     // 2.5 ms
    5_000_000,     // 5 ms
    10_000_000,    // 10 ms
    25_000_000,    // 25 ms
    50_000_000,    // 50 ms
    100_000_000,   // 100 ms
    250_000_000,   // 250 ms
    500_000_000,   // 500 ms
    1_000_000_000, // 1 s
];
const BUCKET_LE_SECONDS: [&str; 13] = [
    "0.0001", "0.00025", "0.0005", "0.001", "0.0025", "0.005", "0.01", "0.025", "0.05", "0.1",
    "0.25", "0.5", "1.0",
];

#[derive(Default)]
pub struct Metrics {
    queries_total: AtomicU64,
    query_errors_total: AtomicU64,
    capsules_returned_total: AtomicU64,
    candidates_scored_total: AtomicU64,
    inserts_total: AtomicU64,
    deletes_total: AtomicU64,
    latency_sum_ns: AtomicU64,
    // Non-cumulative per-bucket counts; rendered cumulatively.
    buckets: [AtomicU64; 13],
    // Per-tenant usage. A brief Mutex (no .await held, no I/O inside) is fine
    // next to a multi-ms query; cardinality is capped by MAX_TRACKED_TENANTS.
    tenants: Mutex<HashMap<u64, TenantUsage>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful query: returned count, scored-candidate count, and
    /// wall-clock latency in nanoseconds.
    pub fn record_query(&self, returned: usize, candidates_scored: usize, latency_ns: u64) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        self.capsules_returned_total
            .fetch_add(returned as u64, Ordering::Relaxed);
        self.candidates_scored_total
            .fetch_add(candidates_scored as u64, Ordering::Relaxed);
        self.latency_sum_ns.fetch_add(latency_ns, Ordering::Relaxed);
        let idx = BUCKET_BOUNDS_NS
            .iter()
            .position(|&b| latency_ns <= b)
            .unwrap_or(BUCKET_BOUNDS_NS.len() - 1);
        self.buckets[idx].fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.query_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_insert(&self) {
        self.inserts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_delete(&self) {
        self.deletes_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn queries(&self) -> u64 {
        self.queries_total.load(Ordering::Relaxed)
    }

    fn with_tenant(&self, tenant: u64, update: impl FnOnce(&mut TenantUsage)) {
        let mut tenants = self.tenants.lock().unwrap_or_else(|e| e.into_inner());
        let key = if tenants.contains_key(&tenant) || tenants.len() < MAX_TRACKED_TENANTS {
            tenant
        } else {
            OVERFLOW_TENANT
        };
        update(tenants.entry(key).or_default());
    }

    /// Attribute a query (and its returned-capsule count) to a tenant.
    pub fn record_query_for(&self, tenant: u64, returned: usize) {
        self.with_tenant(tenant, |usage| {
            usage.queries += 1;
            usage.capsules_returned += returned as u64;
        });
    }

    /// Attribute an insert/update to a tenant.
    pub fn record_insert_for(&self, tenant: u64) {
        self.with_tenant(tenant, |usage| usage.inserts += 1);
    }

    /// Attribute a delete to a tenant.
    pub fn record_delete_for(&self, tenant: u64) {
        self.with_tenant(tenant, |usage| usage.deletes += 1);
    }

    /// Snapshot of per-tenant usage, sorted by tenant id. The overflow bucket
    /// (if any) appears last as `u64::MAX`.
    pub fn usage_snapshot(&self) -> Vec<(u64, TenantUsage)> {
        let tenants = self.tenants.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<(u64, TenantUsage)> = tenants.iter().map(|(k, v)| (*k, *v)).collect();
        rows.sort_by_key(|(tenant, _)| *tenant);
        rows
    }

    /// Render the Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let queries = load(&self.queries_total);
        let mut out = String::new();

        for (name, help, value) in [
            (
                "l5m_queries_total",
                "Total retrieval queries served.",
                queries,
            ),
            (
                "l5m_query_errors_total",
                "Total queries that returned an error.",
                load(&self.query_errors_total),
            ),
            (
                "l5m_capsules_returned_total",
                "Total capsules returned across all queries.",
                load(&self.capsules_returned_total),
            ),
            (
                "l5m_candidates_scored_total",
                "Total candidates scored across all queries.",
                load(&self.candidates_scored_total),
            ),
            (
                "l5m_inserts_total",
                "Total capsules inserted/updated.",
                load(&self.inserts_total),
            ),
            (
                "l5m_deletes_total",
                "Total capsules tombstoned.",
                load(&self.deletes_total),
            ),
        ] {
            out.push_str(&format!(
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {value}\n"
            ));
        }

        out.push_str("# HELP l5m_query_latency_seconds Query latency.\n");
        out.push_str("# TYPE l5m_query_latency_seconds histogram\n");
        let mut cumulative = 0u64;
        for (i, le) in BUCKET_LE_SECONDS.iter().enumerate() {
            cumulative += load(&self.buckets[i]);
            out.push_str(&format!(
                "l5m_query_latency_seconds_bucket{{le=\"{le}\"}} {cumulative}\n"
            ));
        }
        out.push_str(&format!(
            "l5m_query_latency_seconds_bucket{{le=\"+Inf\"}} {queries}\n"
        ));
        let sum_seconds = load(&self.latency_sum_ns) as f64 / 1e9;
        out.push_str(&format!("l5m_query_latency_seconds_sum {sum_seconds}\n"));
        out.push_str(&format!("l5m_query_latency_seconds_count {queries}\n"));

        // Per-tenant usage (metering). Overflow bucket labeled "other".
        let rows = self.usage_snapshot();
        if !rows.is_empty() {
            for (name, help, pick) in [
                (
                    "l5m_tenant_queries_total",
                    "Queries served, by tenant.",
                    0usize,
                ),
                (
                    "l5m_tenant_capsules_returned_total",
                    "Capsules returned, by tenant.",
                    1,
                ),
                (
                    "l5m_tenant_inserts_total",
                    "Capsules inserted/updated, by tenant.",
                    2,
                ),
                (
                    "l5m_tenant_deletes_total",
                    "Capsules tombstoned, by tenant.",
                    3,
                ),
            ] {
                out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} counter\n"));
                for (tenant, usage) in &rows {
                    let label = if *tenant == OVERFLOW_TENANT {
                        "other".to_string()
                    } else {
                        tenant.to_string()
                    };
                    let value = match pick {
                        0 => usage.queries,
                        1 => usage.capsules_returned,
                        2 => usage.inserts,
                        _ => usage.deletes,
                    };
                    out.push_str(&format!("{name}{{tenant=\"{label}\"}} {value}\n"));
                }
            }
        }
        out
    }
}
