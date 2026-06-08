use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct MemoryProbe {
    pub query_text: String,
    pub tenant_id: u64,
    pub as_of: i64,
    pub context_mask: u128,
    pub caller_policy_mask: u128,
    pub trust_floor: u8,
    pub semantic_bits: [u64; 4],
    pub residual: [i8; 64],
    /// Optional dense query embedding (computed offline by the caller). When set
    /// and capsules carry embeddings, retrieval fuses dense similarity with the
    /// lexical/fingerprint ranking. Empty = pure lexical (unchanged behavior).
    pub embedding: Vec<f32>,
    pub anchors: Vec<String>,
    pub entities: Vec<String>,
    pub include_supporting: bool,
    pub include_contradictions: bool,
    pub max_hops: u8,
    pub max_capsules: usize,
    pub max_tokens: usize,
}

impl MemoryProbe {
    pub fn build(
        query_text: &str,
        tenant_id: u64,
        as_of: i64,
        context_mask: u128,
        caller_policy_mask: u128,
        trust_floor: u8,
    ) -> Self {
        let anchors = extract_terms(query_text);
        let entities = anchors.clone();
        let semantic_bits = semantic_bits_for_terms(&anchors);
        let residual = residual_for_text(query_text);
        Self {
            query_text: query_text.to_string(),
            tenant_id,
            as_of,
            context_mask,
            caller_policy_mask,
            trust_floor,
            semantic_bits,
            residual,
            embedding: Vec::new(),
            anchors,
            entities,
            include_supporting: false,
            include_contradictions: false,
            max_hops: 1,
            max_capsules: 8,
            max_tokens: 1024,
        }
    }
}

pub fn extract_terms(text: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    let normalized = text.to_ascii_lowercase();
    let mut token = String::new();
    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':' | '/' | '@') {
            token.push(ch);
        } else if !token.is_empty() {
            maybe_push_term(&mut terms, &token);
            token.clear();
        }
    }
    if !token.is_empty() {
        maybe_push_term(&mut terms, &token);
    }
    terms.into_iter().collect()
}

fn maybe_push_term(terms: &mut BTreeSet<String>, token: &str) {
    let trimmed = token
        .trim_matches(|ch: char| matches!(ch, '.' | '-' | '_' | ':' | '/' | '@') || !ch.is_ascii());
    if trimmed.len() >= 3 {
        terms.insert(trimmed.to_string());
    }
}

pub fn semantic_bits_for_text(text: &str) -> [u64; 4] {
    semantic_bits_for_terms(&extract_terms(text))
}

pub fn semantic_bits_for_terms(terms: &[String]) -> [u64; 4] {
    let mut bits = [0u64; 4];
    for feature in semantic_features(terms) {
        let hash = blake3::hash(feature.as_bytes());
        for chunk in 0..4 {
            let byte = hash.as_bytes()[chunk * 2];
            let bit = byte as usize % 256;
            bits[bit / 64] |= 1u64 << (bit % 64);
        }
    }
    bits
}

pub fn residual_for_text(text: &str) -> [i8; 64] {
    let terms = extract_terms(text);
    residual_for_terms(&terms)
}

pub fn residual_for_terms(terms: &[String]) -> [i8; 64] {
    let mut acc = [0i16; 64];
    for feature in semantic_features(terms) {
        let hash = blake3::hash(feature.as_bytes());
        let bytes = hash.as_bytes();
        let index = bytes[0] as usize % 64;
        let sign = if bytes[1] & 1 == 0 { 1 } else { -1 };
        let weight = 1 + (bytes[2] % 3) as i16;
        acc[index] += sign * weight;
    }
    let mut out = [0i8; 64];
    for (dst, value) in out.iter_mut().zip(acc) {
        *dst = value.clamp(i8::MIN as i16, i8::MAX as i16) as i8;
    }
    out
}

fn semantic_features(terms: &[String]) -> Vec<String> {
    let mut features = terms.to_vec();
    for window in terms.windows(2) {
        features.push(format!("{} {}", window[0], window[1]));
    }
    for window in terms.windows(3) {
        features.push(format!("{} {} {}", window[0], window[1], window[2]));
    }
    features
}
