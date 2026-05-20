# Security Policy

L5M treats memory content as data, never as instructions. Memory can include hostile text, stale policy, low-trust notes, or tenant-private data; those records must not reach an agent unless they pass hard gates.

## Supported Status

This repository is an early Rust MVP. Report security issues privately before public disclosure.

## Security Model

L5M applies these gates before semantic scoring:

- tenant match
- context mask intersection
- policy mask intersection
- temporal validity
- trust floor

The proof-bearing `MemoryFrame` includes selected claims, evidence, trust, validity, source hashes, conflicts, and coverage counts. Expired or contradicted memories can appear in `conflicts` when requested, but they must not appear as normal answer capsules unless they pass all hard gates.

## Threats L5M Is Designed To Reduce

- cross-tenant memory leakage
- stale memory recall
- low-trust memory outranking approved sources
- prompt-injection-like memory being treated as instructions
- dev/lab policy being used in production context
- unsupported answers without evidence metadata

## Reporting

Send a private report to the repository owner with:

- affected commit or release
- reproduction steps
- expected vs actual behavior
- whether the issue leaks tenant, policy, trust, temporal, or source metadata

Do not include real secrets or private customer data in reports.

