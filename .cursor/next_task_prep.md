# Next Task Preparation (Drift-Free)

## Active execution

- **task**: next task selection pending after `TASK-0047` host-first closure.
- **completed task**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`
- **completed contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `Done`

## Carry-in from completed TASK-0047

- [x] `policyd` remains the single policy decision authority.
- [x] Active authoring root is `policies/nexus.policy.toml`.
- [x] `recipes/policy/` is not a live TOML authority.
- [x] `userspace/policy/` owns canonical tree loading, `PolicyVersion`, bounded explain/evaluator semantics, and stable reject codes.
- [x] `policies/manifest.json` is required deterministic tree-hash evidence for `nx policy validate`.
- [x] Config v1 carries policy candidate roots as `policy.root`; `policyd` reload candidates flow through a `configd::ConfigConsumer` 2PC seam.
- [x] External `policyd` host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by `PolicyAuthority` and bounded audit events.
- [x] `policyd` service-facing check frames evaluate through `PolicyAuthority`.
- [x] `nx policy mode` is host preflight-only until a live daemon mode RPC exists.
- [x] `nx policy` is under `tools/nx`; no `nx-*` drift.
- [x] Host proof floor is green:
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`

## Follow-up guardrails

- Do not claim OS/QEMU policy closure until OS-lite reload wiring and OS-facing adapter markers exist.
- Do not add file polling or a policy-specific reload authority; policy candidates must continue to flow as configd-fed candidates.
- Do not reintroduce live policy TOML under `recipes/policy/`.
- Any new policy domain must add behavior-first `test_reject_*` coverage and adapter parity before cutover claims.
