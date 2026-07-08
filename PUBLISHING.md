# Publishing runbook

Maintainer notes for cutting a release: crates.io (Rust), the tagged
signed-binary release, and the language SDKs. All packaging metadata is
already in place — this is the ordered checklist to actually ship.

## Pre-flight (once per release)

- [ ] `main` is green in CI (test matrix, clippy, fmt, MSRV, fuzz, deny).
- [ ] `cargo test --workspace` passes locally; `cargo fmt --all --check` clean.
- [ ] Versions bumped consistently (all workspace crates share `0.1.0` today;
      bump together or move to independent versions deliberately).
- [ ] `CHANGELOG.md` updated.
- [ ] `./scripts/demo.sh` runs clean on a fresh checkout.

## 1. crates.io (Rust)

Publish **in dependency order** — `l5m-core` first; the others pin
`l5m-core = { version = "0.1.0", … }` and won't resolve until it's live.
`l5m-bench` and `l5m-benchmarks` are `publish = false` (internal harnesses).

```bash
# One-time: authenticate (get a token at https://crates.io/settings/tokens)
cargo login <YOUR_CRATES_IO_TOKEN>

# Verify packaging first (no upload):
cargo publish --dry-run -p l5m-core

# Publish, waiting for the index between each (core must be live before deps):
cargo publish -p l5m-core
#   … wait ~30-60s for the crates.io index to update …
cargo publish -p l5m-cli
cargo publish -p l5m-server
cargo publish -p l5m-mcp
```

Name availability is not guaranteed until you publish; if `l5m-core` is taken,
rename the package (`name = "…"`, keep the lib target name or adjust) and
update the inter-crate `path`+`version` deps to match. `docs.rs` builds
automatically after a successful publish.

> Note: the `l5m-core` **default features** compile with zero optional deps.
> The `encryption` feature (ChaCha20-Poly1305) is opt-in; docs.rs renders all
> features via `[package.metadata.docs.rs]` if you add one later.

## 2. Signed binary release (GitHub Actions)

Tagging `v*` triggers `.github/workflows/release.yml`, which builds
cross-platform binaries (`l5m`, `l5m-server`, `l5m-mcp`), SHA-256 checksums,
CycloneDX + SPDX SBOMs, and keyless cosign signatures.

```bash
git tag v0.1.0
git push origin v0.1.0
```

Verify an artifact after the workflow finishes:

```bash
cosign verify-blob \
  --certificate <file>.cert --signature <file>.sig \
  --certificate-identity-regexp 'https://github.com/jbrorepo/l5m/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  <file>
```

## 3. Python SDK (PyPI)

```bash
cd clients/python
python -m build            # produces dist/*.whl and *.tar.gz
python -m twine upload dist/*    # needs a PyPI token
```

## 4. TypeScript SDK (npm)

```bash
cd clients/typescript
npm ci && npm test && npm run build
npm publish --access public      # needs an npm login; package name: l5m-client
```

## Post-release

- [ ] Announce (see `docs/LAUNCH_POSTS.md`): Show HN, r/rust, This Week in Rust.
- [ ] Confirm `docs.rs/l5m-core` rendered.
- [ ] Update the README install snippets if any crate/package name changed.
