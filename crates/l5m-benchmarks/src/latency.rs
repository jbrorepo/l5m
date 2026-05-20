use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StageTimings {
    pub build_or_load_ns: u64,
    pub probe_build_ns: u64,
    pub gate_filter_ns: u64,
    pub anchor_lookup_ns: u64,
    pub semantic_score_ns: u64,
    pub relation_expand_ns: u64,
    pub frame_assembly_ns: u64,
    pub total_retrieval_ns: u64,
}

#[derive(Clone, Debug, Default)]
pub struct LatencySummary {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub p999: u64,
}

pub fn summarize(values: &[u64]) -> LatencySummary {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    LatencySummary {
        p50: percentile(&sorted, 50.0),
        p95: percentile(&sorted, 95.0),
        p99: percentile(&sorted, 99.0),
        p999: percentile(&sorted, 99.9),
    }
}

fn percentile(sorted: &[u64], percentile: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f64 * percentile / 100.0).ceil() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {}
