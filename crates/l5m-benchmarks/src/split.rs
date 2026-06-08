use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SplitFile {
    pub seed: u64,
    pub dev_ids: Vec<String>,
    pub held_out_ids: Vec<String>,
}

pub fn create_split(query_ids: &[String], dev_size: usize, seed: u64) -> SplitFile {
    let mut ids = query_ids.to_vec();
    ids.sort();
    ids.sort_by_key(|id| stable_shuffle_key(id, seed));
    let dev_size = dev_size.min(ids.len());
    let mut dev_ids = ids[..dev_size].to_vec();
    let mut held_out_ids = ids[dev_size..].to_vec();
    dev_ids.sort();
    held_out_ids.sort();
    SplitFile {
        seed,
        dev_ids,
        held_out_ids,
    }
}

pub fn filter_items(
    items: Vec<crate::adapters::BenchmarkItem>,
    split: &SplitFile,
    dev_only: bool,
    held_out: bool,
) -> Vec<crate::adapters::BenchmarkItem> {
    if !dev_only && !held_out {
        return items;
    }
    let allowed = if dev_only {
        &split.dev_ids
    } else {
        &split.held_out_ids
    };
    items
        .into_iter()
        .filter(|item| allowed.contains(&item.query_id))
        .collect()
}

fn stable_shuffle_key(id: &str, seed: u64) -> u64 {
    let mut hash = seed ^ 0x9e37_79b9_7f4a_7c15;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 27;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_out_split_is_deterministic_with_seed_42() {
        let ids = (0..100)
            .map(|index| format!("q{index}"))
            .collect::<Vec<_>>();
        let first = create_split(&ids, 50, 42);
        let second = create_split(&ids, 50, 42);

        assert_eq!(first, second);
        assert_eq!(first.dev_ids.len(), 50);
        assert_eq!(first.held_out_ids.len(), 50);
    }
}
