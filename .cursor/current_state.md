# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-27)**: TASK-0054 / RFC-0046 closure-gap remediation is complete enough for review state: `TASK-0054` is `In Review`, `RFC-0046` is `Done`. The claim remains a narrow Gate E host renderer proof floor, not OS/QEMU present or Gate A kernel/core production-grade closure.
- **active boundary**: Config v1 authority is locked and becomes mandatory carry-in for Policy as Code:
  - Cap'n Proto remains canonical for runtime/persistence config snapshots,
  - JSON remains authoring/validation plus derived CLI/debug view only,
  - deterministic layering stays `defaults < /system < /state < env`,
  - `configd` owns deterministic reload/version transitions and honest 2PC semantics.
- **gate tier**: next UI work remains on Gate E (`Windowing, UI & Graphics`, `production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`. Kernel/core production-grade UI work remains delegated to `TASK-0054B` / `TASK-0054C` / `TASK-0054D`, then `TASK-0288` / `TASK-0290`.

## Active execution state

- **active_task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `In Review`
- **completed_contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`
- **next_queue_head**: `TASK-0054` review pending; `TASK-0055` prep follows after review closure. Do not infer OS/QEMU present closure from TASK-0054.
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

## TASK-0054 review evidence

- `TASK-0054` header now carries follow-ups `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0169`, and `TASK-0170`.
- `TASK-0054` links `RFC-0046`; TASK-0054 remains the execution/proof SSOT, RFC-0046 owns contract/invariants.
- Narrow route chosen: `Frame`, BGRA8888 primitives, deterministic repo-owned fixture-font text, and bounded `Damage`; `TASK-0169` was not promoted.
- Implemented `userspace/ui/renderer/` with `#![forbid(unsafe_code)]`, checked newtypes, owned frame buffers, no global mutable renderer state, no unsafe `Send`/`Sync`, and no host font discovery.
- Implemented `tests/ui_host_snap/` with expected-pixel tests, deterministic snapshot/golden comparison, metadata-independent PNG artifact proof, and required `test_reject_*` cases.
- Current green proof floor after closure-gap remediation:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture` — 24 tests.
  - `cargo test -p ui_host_snap reject -- --nocapture` — 14 reject-filtered tests (`GoldenMode::Update` positive proof is intentionally not matched by the reject filter).
  - `just diag-host`
- No OS/QEMU present markers, kernel/compositor/windowd, GPU/device, scheduler, MM, IPC, VMO, or timer changes were introduced or claimed.
- Closure-gap tests added during review:
  - rounded-rect now uses a full expected-mask proof instead of sentinel pixels only,
  - fixture-font text now uses a full deterministic output mask and rejects unsupported glyphs / too-long glyph runs,
  - blit clipping at the destination edge and padded source stride are proven,
  - exact buffer-length accept/reject behavior, oversized heights, and malformed fixture fonts are proven,
  - safe `GoldenMode::Update` writes under an explicit test artifact root, while compare-only and escape paths reject,
  - PNG artifacts now go under `target/ui_host_snap_artifacts/<pid>` and artifact path traversal rejects,
  - host renderer/snapshot sources are scanned for forbidden fake OS proof markers.
- Review-state sync:
  - `Cargo.lock` is authorized to carry the generated workspace package metadata for `ui_renderer` / `ui_host_snap`,
  - `RFC-0046` is `Done`; `TASK-0054` is `In Review`,
  - downstream status files are synchronized to review state rather than final Done state.
- Later task expectations remain outside TASK-0054 closure and must not be claimed here:
  - `TASK-0055` owns real `windowd`, VMO-backed surfaces, present markers, and compositor behavior,
  - `TASK-0169` may absorb this narrow renderer into Scene-IR / Backend abstractions,
  - `TASK-0054B/C/D`, `TASK-0288`, and `TASK-0290` own kernel QoS/IPC/MM/zero-copy production-grade claims.

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
