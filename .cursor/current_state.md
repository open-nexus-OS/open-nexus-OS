# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-27)**: TASK-0054 / RFC-0046 are `In Progress`. TASK-0054 is explicitly Gate E `production-floor` host-first UI renderer work, not Gate A kernel/core `production-grade` closure; RFC-0046 still requires production-grade local hardening for bounds, ownership, type safety, and proof honesty.
- **active boundary**: Config v1 authority is locked and becomes mandatory carry-in for Policy as Code:
  - Cap'n Proto remains canonical for runtime/persistence config snapshots,
  - JSON remains authoring/validation plus derived CLI/debug view only,
  - deterministic layering stays `defaults < /system < /state < env`,
  - `configd` owns deterministic reload/version transitions and honest 2PC semantics.
- **gate tier**: active execution prep now sits on Gate E (`Windowing, UI & Graphics`, `production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`. Kernel/core production-grade UI work remains delegated to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, then `TASK-0288` / `TASK-0290`.

## Active execution state

- **active_task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `In Progress`
- **active_contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `In Progress`
- **completed_predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`
- **completed_predecessor_contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `Done`

## Locked carry-in constraints from TASK-0046

- Kernel untouched.
- Canonical config authority stays in `nexus-config` + `configd` + `nx config`.
- Layered config authoring under `/system/config` and `/state/config` is JSON-only.
- `nx config push` writes deterministic state overlay `state/config/90-nx-config.json`.
- Marker-only evidence remains insufficient for any future OS/QEMU closure claims.

## TASK-0047 host-first foundation

- `policyd` remains the single policy authority; no second daemon/compiler/CLI was introduced.
- Active policy root is `policies/nexus.policy.toml`; `recipes/policy/` is legacy documentation only and contains no live TOML authority.
- `userspace/policy/` owns canonical tree loading, stable `PolicyVersion`, bounded evaluator semantics, stable reject classes, and adapter parity tests.
- `policyd` stages already-validated `PolicyTree` candidates from Config v1 `policy.root` effective snapshots through the `configd::ConfigConsumer` 2PC seam; invalid candidates do not replace the active version.
- `nx policy` lives only under `tools/nx/`.

## TASK-0054 prep guardrails

- `TASK-0054` header now carries follow-ups `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, and `TASK-0170`.
- `TASK-0054` now links `RFC-0046`; TASK-0054 remains the execution/proof SSOT, RFC-0046 owns contract/invariants.
- Current repo state is recorded in the task: `userspace/ui/renderer/` does not exist; `TASK-0169` / `TASK-0170` remain `Draft`.
- Security section is present for host renderer risks: resource exhaustion, golden update abuse, fixture traversal, malformed dimensions/stride, and future authority drift.
- Red flags must be resolved before implementation: `TASK-0169` overlap, deterministic font fixture, PNG/golden determinism, protected root `Cargo.toml`, and production-grade claim boundary.
- Primary proof target is `cargo test -p ui_host_snap`; no OS/QEMU present markers or kernel/compositor claims are in scope.
- RFC-0046 requires Rust discipline for implementation: checked newtypes for dimensions/stride/damage counts, `#[must_use]` validation/error outcomes, safe ownership, no unsafe `Send`/`Sync`, no `unwrap`/`expect` on inputs, and `#![forbid(unsafe_code)]` for the host renderer crate unless a later RFC permits a low-level backend exception.
- If TASK-0054 exposes simplistic scheduler, MM, IPC, VMO, or timer assumptions, stop and report; route to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, `TASK-0288`, `TASK-0290`, or a new RFC/task rather than weakening the host renderer contract.
- `.cursor/context_bundles.md`, `.cursor/pre_flight.md`, and `.cursor/stop_conditions.md` now include TASK-0054-specific context, gates, and stop conditions for implementation.

## TASK-0047 closure gaps remediated host-first

- `configd` reload lifecycle is now a real host integration seam for policy candidates: tests fail if `PolicyConfigConsumer` ignores the `EffectiveSnapshot`.
- `policyd` exposes/test-proves external host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` backed by `PolicyAuthority`.
- Mode/eval/reload lifecycle audit events are represented for allow, deny, and reject outcomes.
- `policies/manifest.json` records the deterministic tree hash; `nx policy validate` rejects missing or mismatched manifests.
- The `policyd` service-facing check frame evaluates through `PolicyAuthority`; parity tests remain in place for legacy-vs-unified behavior.
- `nx policy mode` is explicitly host preflight-only until a live daemon mode RPC exists.
- OS/QEMU policy markers remain gated and unclaimed; do not use them for closure.

## Proven carry-in evidence (TASK-0046)

- Host proof floor is green:
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
- Required proof classes are covered:
  - schema rejects: unknown/type/depth/size fail closed with stable classification,
  - lexical-order layer directory merge + deterministic precedence,
  - byte-identical Cap'n Proto snapshots for equivalent inputs,
  - 2PC reject/timeout/commit-failure keeps prior version active,
  - `nx config` deterministic exit and `--json` contracts,
  - `nx config effective --json` parity with `configd` version + derived JSON for the same layered inputs.

## Proven host evidence so far (TASK-0047)

- `cargo test -p policy -- --nocapture` — green, 18 tests.
- `cargo test -p nexus-config -- --nocapture` — green, 10 tests.
- `cargo test -p configd -- --nocapture` — green, 8 tests.
- `cargo test -p policyd -- --nocapture` — green, 25 tests.
- `cargo test -p nx -- --nocapture` — green, 23 unit tests + 8 CLI contract tests.
- OS/QEMU policy markers remain gated and unclaimed.

## Follow-up split (preserve scope)

- `TASK-0047`: Policy as Code v1 on top of Config v1 authority.
- `TASK-0262`: determinism/hygiene floor alignment and anti-fake-success discipline.
- `TASK-0266`: single-authority and naming contract continuity.
- `TASK-0268`: `nx` convergence, no `nx-*` logic drift.
- `TASK-0273`: downstream consumer adoption without parallel config authority.
- `TASK-0285`: QEMU harness phase/failure evidence discipline for OS-gated closure.
