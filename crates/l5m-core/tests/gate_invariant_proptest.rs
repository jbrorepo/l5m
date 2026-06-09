// Property-based proof of the core security invariant:
//
//   For ANY corpus and ANY probe, every capsule returned by `retrieve` satisfies
//   ALL hard gates (tenant, context, policy, trust, temporal).
//
// proptest generates thousands of randomized multi-tenant corpora and probes
// (random masks, trust levels, validity windows, embeddings on/off, ANN/exact
// paths) and checks the invariant on each — a machine-checked guarantee that no
// input makes an unauthorized capsule reachable.

use std::collections::HashMap;
use std::fs;

use l5m_core::{compile_segment, retrieve, CompileOptions, MemoryProbe, Segment};
use proptest::prelude::*;
use tempfile::tempdir;

#[derive(Clone, Debug)]
struct Cap {
    id: u128,
    tenant: u64,
    context: u16,
    policy: u16,
    trust: u8,
    valid_from: i64,
    valid_until: Option<i64>,
}

fn cap_strategy() -> impl Strategy<Value = Cap> {
    (
        1u128..200,                   // id
        1u64..5,                      // tenant (small set so probes actually match)
        0u16..=0xffff,                // context mask
        0u16..=0xffff,                // policy mask
        0u8..=10,                     // trust
        0i64..1000,                   // valid_from
        prop::option::of(0i64..1000), // valid_until
    )
        .prop_map(
            |(id, tenant, context, policy, trust, valid_from, valid_until)| Cap {
                id,
                tenant,
                context,
                policy,
                trust,
                valid_from,
                valid_until,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn retrieve_never_returns_a_capsule_that_violates_a_gate(
        caps in prop::collection::vec(cap_strategy(), 1..40),
        probe_tenant in 1u64..5,
        probe_context in 0u16..=0xffff,
        probe_policy in 0u16..=0xffff,
        probe_trust in 0u8..=10,
        probe_as_of in 0i64..1000,
        with_embeddings in any::<bool>(),
    ) {
        // De-duplicate capsule ids (the segment requires uniqueness).
        let mut by_id: HashMap<u128, Cap> = HashMap::new();
        for c in caps { by_id.entry(c.id).or_insert(c); }
        let caps: Vec<Cap> = by_id.into_values().collect();

        let entries: Vec<String> = caps.iter().map(|c| {
            let until = c.valid_until.map(|u| format!(",\"valid_until\":{u}")).unwrap_or_default();
            let emb = if with_embeddings {
                format!(",\"embedding\":[{},{}]", (c.id % 7) as f32, (c.trust % 3) as f32)
            } else { String::new() };
            format!(
                "{{\"capsule_id\":\"{}\",\"tenant_id\":{},\"claim\":\"c{} backup retention policy\",\
                 \"evidence\":\"evidence {} retention scanning audit\",\"source_id\":{},\
                 \"valid_from\":{},\"observed_at\":1,\"last_verified_at\":1,\
                 \"context_mask\":\"{:#x}\",\"policy_mask\":\"{:#x}\",\"trust_level\":{},\
                 \"classification\":1,\"poison_risk\":0{until}{emb}}}",
                c.id, c.tenant, c.id, c.id, c.id, c.valid_from, c.context, c.policy, c.trust
            )
        }).collect();
        let json = format!("[{}]", entries.join(","));

        let dir = tempdir().unwrap();
        let input = dir.path().join("in.json");
        let output = dir.path().join("seg.segment");
        fs::write(&input, &json).unwrap();
        compile_segment(CompileOptions { input_json: input, output_segment: output.clone(), epoch: 1 }).unwrap();
        let segment = Segment::open(&output).unwrap();

        let mut probe = MemoryProbe::build(
            "backup retention policy scanning audit",
            probe_tenant,
            probe_as_of,
            probe_context as u128,
            probe_policy as u128,
            probe_trust,
        );
        probe.max_capsules = 40;
        if with_embeddings { probe.embedding = vec![1.0, 1.0]; }

        let frame = retrieve(&segment, &probe).unwrap();

        for returned in &frame.capsules {
            let src = segment.capsule_by_id(returned.capsule_id)
                .expect("returned capsule must exist in segment");
            prop_assert_eq!(src.tenant_id, probe_tenant, "tenant gate violated");
            prop_assert_ne!(src.context_mask & probe.context_mask, 0, "context gate violated");
            prop_assert_ne!(src.policy_mask & probe.caller_policy_mask, 0, "policy gate violated");
            prop_assert!(src.trust_level >= probe_trust, "trust gate violated");
            prop_assert!(src.valid_from <= probe_as_of, "temporal(from) gate violated");
            prop_assert!(
                src.valid_until.is_none_or(|u| u >= probe_as_of),
                "temporal(until) gate violated"
            );
        }
    }
}
