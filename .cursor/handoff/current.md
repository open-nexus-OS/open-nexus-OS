# Current Handoff: TASK-0047 done host-first

**Date**: 2026-04-26  
**Completed execution task**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `Done`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate B: Security, Policy & Identity, `production-grade`)

## Closure summary

- Phase 0 `tools/nx` refactor is complete and behavior-preserving.
- Active policy tree authority is `policies/nexus.policy.toml`.
- `recipes/policy/` is no longer a live TOML authority.
- `userspace/policy/` provides canonical tree loading, deterministic `PolicyVersion`, bounded evaluator decisions, and stable reject codes.
- Config v1 carries policy candidate roots in `policy.root`; `policyd` consumes those effective snapshots through `configd::ConfigConsumer`.
- External `policyd` host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by `PolicyAuthority` and bounded audit events.
- `policyd` service-facing check frames evaluate through `PolicyAuthority`.
- `policies/manifest.json` records the deterministic tree hash and `nx policy validate` rejects missing or stale manifests.
- `nx policy mode` is host preflight-only until a live daemon mode RPC exists.
- `nx policy validate|diff|explain|mode` lives under `tools/nx` only.

## Proof evidence

- `cargo test -p policy -- --nocapture` — green, 18 tests.
- `cargo test -p nexus-config -- --nocapture` — green, 10 tests.
- `cargo test -p configd -- --nocapture` — green, 8 tests.
- `cargo test -p policyd -- --nocapture` — green, 25 tests.
- `cargo test -p nx -- --nocapture` — green, 23 unit tests + 8 CLI contract tests.

## Carry-forward guardrails

- Kernel untouched.
- OS/QEMU policy markers remain gated and unclaimed.
- No `nx-*` drift was introduced.
- No second policy daemon/compiler/live root was introduced.
- Follow-up OS runtime wiring must continue to feed policy candidates through `configd` rather than adding file polling or a parallel reload path.
