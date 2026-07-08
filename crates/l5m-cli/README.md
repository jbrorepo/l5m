# l5m-cli

Command-line interface for [L5M](https://github.com/jbrorepo/l5m), the
security-gated memory engine. Installs the `l5m` binary.

```bash
cargo install l5m-cli

# Compile memory capsules (JSON) into a binary segment
l5m compile --input memories.json --output memories.segment --epoch 1

# Run a gated query (tenant/policy/trust/temporal enforced before scoring)
l5m query --segment memories.segment --tenant 7 \
  --query "How long do we retain backups?" --as-of 1770000000 \
  --context-mask 0xffff --policy-mask 0xffff --trust-floor 4 --max-capsules 8
```

Output is a proof-bearing memory frame (claims, evidence, trust levels,
validity windows, source hashes, coverage stats).

Full project and docs: <https://github.com/jbrorepo/l5m>.

## License

MIT OR Apache-2.0
