# L5M Quick Start Guide

Get from zero to your first query in under 10 minutes.

## What You'll Learn

By the end of this guide, you'll:
- Install L5M
- Compile a memory segment
- Run queries with different parameters
- Understand the proof-bearing output
- See gate enforcement in action

**Time required:** ~10 minutes

---

## Step 1: Install L5M (1 minute)

### Option A: From crates.io (recommended)

```bash
cargo install l5m-cli
```

### Option B: From source

```bash
git clone https://github.com/yourusername/l5m.git
cd l5m
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

### Verify installation

```bash
l5m-cli --version
```

You should see: `l5m-cli 0.1.0`

---

## Step 2: Get Example Data (1 minute)

L5M includes example memory data demonstrating all 5 dimensions.

```bash
# Create a working directory
mkdir l5m-demo
cd l5m-demo

# Download example memories
curl -O https://raw.githubusercontent.com/yourusername/l5m/main/examples/seed_memories.json
```

**What's in the example data?**

The `seed_memories.json` file contains memory capsules about a fictional company's policies:
- Database backup retention policies
- Security protocols
- Development environment configurations
- Deprecated/superseded policies

Each capsule has:
- **Claim**: Short statement (what to remember)
- **Evidence**: Detailed context (why/how)
- **Trust level**: 0-10 (source reliability)
- **Validity window**: When this memory is valid
- **Policy mask**: Who can access this
- **Relations**: Links to other memories (supports, contradicts, supersedes)

---

## Step 3: Compile Memory Segment (1 minute)

L5M compiles JSON memories into an immutable binary segment file.

```bash
l5m-cli compile \
  --input seed_memories.json \
  --output demo.segment \
  --epoch 1
```

**Output:**
```
Compiled 12 capsules into demo.segment
Segment size: 8.2 KB
Manifest written to demo.segment.manifest.json
```

**What just happened?**

1. L5M read your JSON capsules
2. Extracted semantic features (anchors, entities, fingerprints)
3. Built relation graph
4. Wrote immutable binary segment
5. Generated manifest with checksums

The segment is now ready for queries. No database, no server, just a file.

---

## Step 4: Your First Query (2 minutes)

Let's query for database backup policies.

```bash
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "How long do we retain production database backups?" \
  --as-of 1770000000 \
  --context-mask 0xffff \
  --policy-mask 0xffff \
  --trust-floor 4 \
  --max-capsules 8
```

**Output (pretty JSON):**

```json
{
  "epoch": 1,
  "query_hash": "a3f2...",
  "capsules": [
    {
      "capsule_id": "01234567-89ab-cdef-0123-456789abcdef",
      "claim": "Production database backups are retained for 35 days",
      "evidence": "Per compliance policy CP-2024-03, production database backups must be retained for 35 days to meet regulatory requirements. Backups older than 35 days are automatically purged.",
      "trust_level": 8,
      "valid_from": 1704067200,
      "valid_until": null,
      "source_id": 12345,
      "source_hash": "b4c8...",
      "score": 12.4,
      "relation_notes": []
    }
  ],
  "conflicts": [],
  "coverage": {
    "context_valid_count": 12,
    "temporal_valid_count": 10,
    "trust_floor_met_count": 8,
    "exact_entity_match": true,
    "anchor_match_count": 3,
    "candidate_count_before_scoring": 6
  }
}
```

**Understanding the output:**

- **capsules**: The retrieved memories (claims + evidence)
- **trust_level**: 8/10 - high confidence source
- **valid_from**: Unix timestamp when this became valid
- **valid_until**: null means still valid
- **source_hash**: Cryptographic proof of source
- **score**: Relevance score (higher = more relevant)
- **coverage**: Shows how gates filtered candidates
  - 12 capsules matched context
  - 10 were temporally valid
  - 8 met trust floor
  - 6 passed all gates and were scored

---

## Step 5: Experiment with Parameters (3 minutes)

### Query at Different Times

```bash
# Query as of January 2024
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "What is our backup policy?" \
  --as-of 1704067200 \
  --trust-floor 4

# Query as of January 2025 (might see superseded policies)
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "What is our backup policy?" \
  --as-of 1735689600 \
  --trust-floor 4
```

**What changed?** Policies with `valid_until` before the query time are excluded.

### Lower Trust Floor

```bash
# Include lower-trust sources
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "What is our backup policy?" \
  --as-of 1770000000 \
  --trust-floor 2 \
  --max-capsules 8
```

**What changed?** More capsules returned, including informal notes and draft policies.

### Include Contradictions

```bash
# Show conflicting information
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "What is our backup policy?" \
  --as-of 1770000000 \
  --trust-floor 4 \
  --include-contradictions
```

**What changed?** The `conflicts` array now includes capsules that contradict the main results, with relation notes explaining the conflict.

---

## Step 6: See Gate Enforcement (2 minutes)

### Tenant Isolation

```bash
# Query as tenant 1 (should work)
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "backup policy" \
  --as-of 1770000000 \
  --trust-floor 4

# Query as tenant 2 (should return nothing - wrong tenant)
l5m-cli query \
  --segment demo.segment \
  --tenant 2 \
  --query "backup policy" \
  --as-of 1770000000 \
  --trust-floor 4
```

**Result:** Tenant 2 gets zero results because all example capsules belong to tenant 1.

### Trust Floor Enforcement

```bash
# High trust only (trust >= 8)
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "backup policy" \
  --as-of 1770000000 \
  --trust-floor 8

# Low trust allowed (trust >= 2)
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "backup policy" \
  --as-of 1770000000 \
  --trust-floor 2
```

**Result:** Higher trust floor returns fewer, more reliable capsules.

---

## Understanding the 5D Model

Every capsule in L5M has coordinates in 5 dimensions:

### 1. Semantic Dimension
- **Anchors**: Key terms extracted from text
- **Entities**: Named entities (IDs, names, concepts)
- **Fingerprint**: 256-bit semantic hash
- **Residual**: 64-element int8 vector

**Query:** "database backup retention"  
**Matches:** Capsules with overlapping anchors/entities

### 2. Temporal Dimension
- **valid_from**: When this memory became true
- **valid_until**: When it was superseded/expired
- **observed_at**: When it was recorded
- **last_verified_at**: When it was last confirmed

**Query:** `--as-of 1770000000`  
**Filters:** Only capsules valid at that timestamp

### 3. Context Dimension
- **tenant_id**: Which organization/user
- **context_mask**: Environment, project, sensitivity, task type

**Query:** `--tenant 1 --context-mask 0xffff`  
**Filters:** Only capsules matching tenant and context

### 4. Relation Dimension
- **Supports**: This memory reinforces another
- **Contradicts**: This memory conflicts with another
- **Supersedes**: This memory replaces another
- **Depends on**: This memory requires another

**Query:** `--include-contradictions`  
**Returns:** Related capsules with relation notes

### 5. Veracity Dimension
- **trust_level**: 0-10 source reliability
- **source_id**: Where this came from
- **policy_mask**: Access control
- **poison_risk**: Potential adversarial content

**Query:** `--trust-floor 4 --policy-mask 0xffff`  
**Filters:** Only trusted, authorized capsules

---

## Next Steps

### Create Your Own Memories

Create `my_memories.json`:

```json
[
  {
    "capsule_id": "00000000-0000-0000-0000-000000000001",
    "tenant_id": 1,
    "claim": "User prefers dark mode",
    "evidence": "User explicitly enabled dark mode in settings on 2024-01-15",
    "source_id": 1001,
    "source_uri": "app://settings/theme",
    "valid_from": 1705276800,
    "observed_at": 1705276800,
    "last_verified_at": 1705276800,
    "context_mask": "0xffff",
    "policy_mask": "0xffff",
    "trust_level": 9,
    "classification": 1,
    "poison_risk": 0
  }
]
```

Compile and query:

```bash
l5m-cli compile --input my_memories.json --output my.segment --epoch 1
l5m-cli query --segment my.segment --tenant 1 --query "user preferences" --as-of 1770000000 --trust-floor 4
```

### Explore Examples

Check out complete example projects:

```bash
cd examples/conversational-memory
cargo run
```

### Read the Docs

- [Architecture Deep Dive](ARCHITECTURE.md) - How L5M works internally
- [API Documentation](https://docs.rs/l5m-core) - Use L5M in your Rust code
- [Deployment Guide](DEPLOYMENT.md) - Production deployment
- [Comparison](COMPARISON.md) - L5M vs alternatives

### Join the Community

- [GitHub Discussions](https://github.com/yourusername/l5m/discussions) - Ask questions
- [GitHub Issues](https://github.com/yourusername/l5m/issues) - Report bugs

---

## Troubleshooting

### "command not found: l5m-cli"

**Solution:** Make sure `~/.cargo/bin` is in your PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### "failed to open segment"

**Solution:** Check the file path is correct:

```bash
ls -lh demo.segment
```

### "no capsules returned"

**Possible causes:**
1. Wrong tenant ID (check capsules are for your tenant)
2. Trust floor too high (lower it to see more results)
3. Query timestamp outside validity window (adjust `--as-of`)
4. No semantic match (try broader query terms)

**Debug:** Look at the `coverage` field in the output to see where capsules were filtered.

### "segment version mismatch"

**Solution:** Recompile the segment with the current L5M version:

```bash
l5m-cli compile --input seed_memories.json --output demo.segment --epoch 1
```

---

## Summary

You've learned:
- ✅ How to install L5M
- ✅ How to compile memory segments
- ✅ How to query with different parameters
- ✅ How to interpret proof-bearing output
- ✅ How gates enforce security before scoring
- ✅ How the 5D model works

**Time to build something awesome!** 🚀

Questions? [Open a discussion](https://github.com/yourusername/l5m/discussions)
