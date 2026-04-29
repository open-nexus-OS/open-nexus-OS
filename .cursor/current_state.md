# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-29)**: `TASK-0055`/`RFC-0047` are closed as `Done`, and active execution focus moved to `TASK-0055B` with new contract seed `RFC-0048` (`Draft`) for visible QEMU scanout bootstrap; `TASK-0055` headless closure remains a strict carry-in baseline and must not be over-claimed as visible output/input closure.
- **active boundary**: Config v1 authority is locked and becomes mandatory carry-in for Policy as Code:
  - Cap'n Proto remains canonical for runtime/persistence config snapshots,
  - JSON remains authoring/validation plus derived CLI/debug view only,
  - deterministic layering stays `defaults < /system < /state < env`,
  - `configd` owns deterministic reload/version transitions and honest 2PC semantics.
- **gate tier**: TASK-0055 contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`, but it does not close all Gate E expectations by itself. Visible scanout, visible SystemUI present, input routing, and kernel/core production-grade UI work remain delegated to follow-ups.

## Active execution state

- **active_task**: `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md` — `Draft`
- **active_contract**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Draft`
- **completed_task**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`
- **completed_task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`
- **next_queue_head**: `TASK-0055B` / visible QEMU scanout bootstrap. Do not infer visible output or input closure from `TASK-0055` headless closure.
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

## TASK-0054 closeout evidence

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
  - `just test-all`
  - `just ci-network` (repo regression gate only; not TASK-0054 OS-present proof)
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`
- No OS/QEMU present markers, kernel/compositor/windowd, GPU/device, scheduler, MM, IPC, VMO, or timer changes were introduced or claimed.
- Closure-gap tests added during review:
  - rounded-rect now uses a full expected-mask proof instead of sentinel pixels only,
  - fixture-font text now uses a full deterministic output mask and rejects unsupported glyphs / too-long glyph runs,
  - blit clipping at the destination edge and padded source stride are proven,
  - exact buffer-length accept/reject behavior, oversized heights, and malformed fixture fonts are proven,
  - safe `GoldenMode::Update` writes under an explicit test artifact root, while compare-only and escape paths reject,
  - PNG artifacts now go under `target/ui_host_snap_artifacts/<pid>` and artifact path traversal rejects,
  - host renderer/snapshot sources are scanned for forbidden fake OS proof markers.
- Closeout sync:
  - `Cargo.lock` is authorized to carry the generated workspace package metadata for `ui_renderer` / `ui_host_snap`,
  - `RFC-0046` is `Done`; `TASK-0054` is `Done`,
  - downstream status files are synchronized to final Done state.
- Docs sync now includes `docs/testing/index.md`, `docs/dev/ui/foundations/quality/goldens.md`,
  `docs/architecture/nexusgfx-compute-and-executor-model.md`, and
  `docs/architecture/nexusgfx-text-pipeline.md`.
- Later task expectations remain outside TASK-0054 closure and must not be claimed here:
  - `TASK-0055` owns real `windowd`, VMO-backed surfaces, present markers, and compositor behavior,
  - `TASK-0169` may absorb this narrow renderer into Scene-IR / Backend abstractions,
  - `TASK-0054B/C/D`, `TASK-0288`, and `TASK-0290` own kernel QoS/IPC/MM/zero-copy production-grade claims.

## TASK-0055 closeout state

- `RFC-0047` is `Done` after remediation; `TASK-0055` is `Done` with execution/proof closure synced.
- Implemented `source/services/windowd/` as focused modules (`error`, `ids`, `geometry`, `buffer`, `frame`, `server`, `markers`, `smoke`, `cli`, `legacy`) behind a small facade. The previous checksum scaffold no longer counts as proof.
- Implemented `tests/ui_windowd_host/` with behavior-first positive tests, generated Cap'n Proto codec/roundtrip proofs, IDL shape checks, and `test_reject_*` coverage for invalid dimensions/stride/format, missing/forged/wrong-rights VMO handles, stale IDs/sequences, unauthorized layer mutation, buffer length mismatch, bounds rejects, invalid damage, no committed scene, vsync subscription, explicit input stub behavior, atomic scene commit preservation, and marker/postflight fake-proof rejection.
- Implemented `userspace/apps/launcher/` as the canonical `launcher` package. The old `source/apps/launcher` placeholder was deleted, and `recipes/apps/launcher/recipe.toml` now points at `userspace/apps/launcher`.
- Wired UI markers into `selftest-client`, proof-manifest, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh`.
- Green proof floor:
  - `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`
  - `cargo test -p ui_windowd_host reject -- --nocapture`
  - `cargo test -p ui_windowd_host capnp -- --nocapture`
  - `cargo test -p selftest-client -- --nocapture`
  - `cargo test -p launcher -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `scripts/fmt-clippy-deny.sh`
  - `make build` → `make test`
  - `make build` → `make run`
- TASK-0055 uses a tiny headless `desktop` proof profile (`64x48`, `60Hz`) to stay within current selftest heap limits.
- No visible scanout, real input routing/focus/click, rich dev presets, GPU/display-driver path, or new kernel/MM/IPC/zero-copy production-grade claim is made.

## TASK-0055 critical closure remediation

- `source/services/windowd/src/lib.rs` monolith risk is remediated; it is now a facade over focused modules.
- Old launcher package conflict is remediated; the canonical package is `userspace/apps/launcher` with package name `launcher`.
- `windowd::render_frame()` remains only in `legacy.rs` for old `compositor` scaffold compatibility. It is still not a TASK-0055 proof and should be removed when `compositor` is migrated.
- Generated Cap'n Proto codec/roundtrip behavior is proven for surface create, queue-buffer damage, layer commit, vsync subscribe, and input subscribe messages.
- VMO behavior is closed as the TASK-0055 UI-shaped boundary: modeled handles carry owner/rights/byte-length/surface-buffer metadata and fail closed. Real kernel VMO capability transfer, sealing/reuse, IPC fastpath, and zero-copy production claims remain owned by TASK-0031 follow-ups / production gates, not by TASK-0055.
- Caller identity is narrowed through `CallerCtx::from_service_metadata`; raw `CallerId::new` is no longer public.
- `SystemUI loaded` remains a minimal headless profile-state proof only; visible SystemUI/process/layer richness remains follow-up scope.
- Launcher coupling is remediated: launcher now owns a client-style first-frame function and tests marker rejection without `PresentAck`.
- Vsync subscription and input-stub Rust APIs/tests now exist for the headless contract.
- Bounds/reject gaps for too many surfaces/layers/damage rects, oversized total bytes, invalid damage, no committed scene, and atomic scene commit preservation are now covered in `ui_windowd_host`.
- Present marker rendering now comes from `PresentAck` evidence; selftest no longer emits a hard-coded present marker.
- `tools/postflight-ui.sh --uart-log` log-only rejection is directly tested; `scripts/qemu-test.sh` still relies on the canonical QEMU run plus inline guard logic rather than isolated synthetic log fixtures.
- The green gates prove scoped TASK-0055 requirements. Broader visible-display/input/perf/kernel-VMO claims remain explicit follow-ups.

## TASK-0055B active prep state

- `TASK-0055B` is the active execution SSOT and remains `Draft` until visible scanout bootstrap proofs are implemented and green.
- `RFC-0048` is the contract seed for this slice and remains `Draft`; it defines visible scanout bootstrap invariants and anti-fake-success marker gating.
- Active claim boundary is narrow: one deterministic visible mode and marker ladder in QEMU graphics mode; no cursor/input/perf/kernel production-grade claims.
- Required marker honesty for this slice is explicit: `display: first scanout ok` and `SELFTEST: display bootstrap visible ok` must only appear after real visible framebuffer write plus harness verification.
- Follow-up ownership remains explicit and unchanged: `TASK-0055C` (visible systemui first frame), `TASK-0251` (display OS integration), `TASK-0056B` (input routing), and kernel/perf follow-ups.

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
