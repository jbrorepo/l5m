# GitHub Launch Checklist

Use this checklist for the first public repository push.

## Before The First Commit

1. Keep generated and third-party benchmark data out of git. The repository
   ignores `data/`, `runs/`, and `external/`; use the benchmark fetch commands
   below to recreate local data.
2. Confirm the license choice is still `MIT OR Apache-2.0`.
3. Run the full local quality gate:

   ```powershell
   cargo fmt
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

4. Review the first commit contents with:

   ```powershell
   git status --short
   git add .
   git status --short
   ```

## Create The GitHub Repository

The public GitHub repository is `https://github.com/jbrorepo/l5m`.

Add it as `origin` when publishing from a fresh local checkout:

```powershell
git remote add origin https://github.com/jbrorepo/l5m.git
git branch -M main
git commit -m "chore: prepare initial l5m release"
git push -u origin main
```

The workspace `repository` metadata in `Cargo.toml` points at the GitHub URL.

## CI

The repository includes `.github/workflows/ci.yml`, which runs:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

Enable branch protection for `main` after the first CI run passes.

## Benchmark Data

The benchmark datasets are intentionally local-only because they are large and
may carry their own upstream licenses.

Fetch ConvoMem into `data/ConvoMem`:

```powershell
.\scripts\fetch-convomem.ps1
```

Place LongMemEval and LoCoMo JSON inputs under `data/` when running benchmark
commands. Generated run rows belong under `runs/`; compact summaries can be
kept under `reports/` when they are useful for review.
