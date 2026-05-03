# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-05-03)**: `TASK-0056B`/`RFC-0051` are `Done` and archived in handoff; `TASK-0252` prep was hardened (dependency, security invariants, red-flag mitigations, Gate-E mapping, touched-path drift fixes) and is now `In Progress`.
- **active boundary**: Config v1 authority is locked and becomes mandatory carry-in for Policy as Code:
  - Cap'n Proto remains canonical for runtime/persistence config snapshots,
  - JSON remains authoring/validation plus derived CLI/debug view only,
  - deterministic layering stays `defaults < /system < /state < env`,
  - `configd` owns deterministic reload/version transitions and honest 2PC semantics.
- **gate tier**: UI closure remains in Gate E (`Windowing, UI & Graphics`, `production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`. `TASK-0056` closes deterministic present scheduler + input-routing baseline; `TASK-0056B` closes deterministic visible cursor/hover/focus/click proof; `TASK-0252`/`TASK-0253` are pulled directly after it for live QEMU input; `TASK-0056C` follows for responsiveness before scroll/animation/launcher UX claims; 0199/0200 plus kernel lanes remain explicit follow-ups.

## Active execution state

- **completed_task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `Done`
- **completed_task**: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` — `Done`
- **completed_task**: `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md` — `Done`
- **active_task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `In Progress`
- **active_contract_seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `In Progress`
- **active_contract_carry_in**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done` (visible bootstrap baseline)
- **completed_task**: `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done`
- **completed_task**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`
- **completed_task**: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` — `Done`
- **completed_contract**: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md` — `Done`
- **next_queue_head**: `TASK-0252` is active (`In Progress`). Do not infer live QEMU pointer/keyboard closure from 56B deterministic scope.
- **completed_predecessor**: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` — `Done`
- **completed_predecessor_contract**: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md` — `Done`

## TASK-0056 implementation state

- `TASK-0056` is `Done`; contract phases for double-buffered present, scheduler/fences, and input routing are implemented and proven with full closure gates green.
- Implemented in the existing `windowd` authority path:
  - frame-indexed back-buffer acquisition and bounded pending present state,
  - deterministic scheduler tick coalescing and post-present minimal fence signaling,
  - committed-layer hit-test, focus-follows-click, keyboard delivery, and bounded input event queues.
- Proofs green so far:
  - closure rerun `cargo test -p windowd -p launcher -p ui_v2a_host -- --nocapture` — 22 tests across the three target packages,
  - closure rerun `cargo test -p ui_v2a_host reject -- --nocapture` — 5 reject-filtered tests,
  - `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`,
  - OS-target `selftest-client` visible-bootstrap build check,
  - closure rerun `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` with `verify-uart` accepted through `SELFTEST: ui v2 input ok`.
- Closure sync completed for touched headers, ADR/architecture/testing docs, task/RFC notes, and marker-honesty gating. `SELFTEST: ui v2 input ok` now requires both real input routing and launcher click evidence.
- Closure gate reruns are green: `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, and `make clean` -> `make build` -> `make test` -> `make run`.
- Scope boundary remains unchanged: no `TASK-0056B` cursor polish, no `TASK-0056C` perf/latency closure, no `TASK-0199`/`TASK-0200` WM-v2 breadth, no kernel production-grade claim, and no independent screenshot/GTK visual-proof claim.

## TASK-0056B implementation state

- `TASK-0056B` is `Done` with `RFC-0051` as `Done`; host/reject/deterministic-QEMU proof phases and closure quality gates are green, and live QEMU input remains the immediate `TASK-0252`/`TASK-0253` follow-up lane.
- Implemented in the existing `windowd` authority path:
  - routed pointer movement records bounded pointer state and produces deterministic visible cursor pixels,
  - routed pointer-down transfers focus and renders deterministic focus affordance pixels,
  - launcher visible-click marker is a proof consumer gated on `windowd` visible input evidence.
- Proofs green so far for the deterministic visible-input route:
  - `cargo test -p ui_v2a_host -- --nocapture` — 19 tests,
  - `cargo test -p ui_v2a_host reject -- --nocapture` — 12 reject-filtered tests,
  - `cargo test -p windowd -p launcher -- --nocapture` — 15 tests,
  - `cargo test -p selftest-client -- --nocapture`,
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` accepted through `SELFTEST: ui visible input ok`.
- Follow-up visual-proof investigation fixed a fake-green root cause: `selftest-client`
  now writes QEMU `etc/ramfb` config fields in the required ABI order
  (`addr, fourcc, flags, width, height, stride`) instead of treating DMA completion as
  sufficient display initialization evidence.
- Human-visibility follow-up: the 64x48 `windowd` visible-input proof is scaled to the
  1280x800 `ramfb` scanout and now writes cursor-start, hover/cursor-end, and final
  focus/click frames before emitting visible-input success.
- Scope correction after review: live host mouse/keyboard in the QEMU GTK window is no longer a 56B requirement.
  Required live-input work moves to `TASK-0252`/`TASK-0253`: proper QEMU pointer/keyboard sources,
  bounded dispatch into `windowd`/IME, visible hover/click state, and markers that cannot be
  satisfied by deterministic selftest injection alone.
- Closure gates now green in sequence: `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, and `make clean` -> `make build` -> `make test` -> `make run`.
- Scope remains explicit: no `TASK-0252`/`TASK-0253` live input pipeline in 56B, no `TASK-0056C` perf/latency, no `TASK-0199`/`TASK-0200` WM/compositor-v2 breadth, no `TASK-0251` display service integration, and no kernel production-grade claim.
- Fast-lane uplift after user review: downstream UI/SystemUI tasks now carry an Orbital-Level UX gate using Open Nexus/OHOS/Zircon-style authorities. Before `TASK-0119`/`TASK-0120` can claim desktop/launcher quality, the lane must prove visible greeter/dev-session, live pointer/keyboard basics (`TASK-0252`/`TASK-0253`), text/IME/OSK basics (`TASK-0146`/`TASK-0147` after `TASK-0059`), scroll, launcher/app-window flow, Quick Settings, and SVG-source UI assets without adopting Orbital architecture.

## TASK-0252 execution state

- `TASK-0252` is `In Progress`; `RFC-0052` is `In Progress` as the active contract seed.
- Contract posture is host-first and test-first:
  - Soll-behavior + `test_reject_*` suites are required before closure claims,
  - marker-only closure is explicitly disallowed for 0252.
- Scope boundary is explicit:
  - in-scope: host core libraries (`hid`, `touch`, `keymaps`, `key-repeat`, `pointer-accel`) and deterministic host tests,
  - out-of-scope: OS/QEMU services, DTB/device wiring, and `nx input` CLI (`TASK-0253`).
- Rust quality floor is mandatory in this slice: newtypes where ambiguity matters, explicit ownership boundaries, no unsafe `Send`/`Sync` shortcuts, and `#[must_use]` for decision-bearing APIs.

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

## TASK-0055B closeout state

- `TASK-0055B` is `Done`; `RFC-0048` is `Done`.
- Implemented one fixed QEMU `ramfb` visible bootstrap path selected by `NEXUS_DISPLAY_BOOTSTRAP=1`; the proof-manifest `visible-bootstrap` profile is a harness/marker profile, not a SystemUI/launcher start profile.
- `nexus-init` grants `selftest-client` a policy-gated `device.mmio.fwcfg` capability; `selftest-client` writes a `1280x800` ARGB8888 framebuffer VMO and configures `etc/ramfb` via `fw_cfg` DMA.
- `windowd` owns fixed-mode validation, deterministic bootstrap pixels, present evidence, and pre-scanout marker gating.
- Green proof floor:
  - `cargo test -p windowd -p ui_windowd_host -- --nocapture`
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo check -p init-lite --target riscv64imac-unknown-none-elf --release`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- Marker ladder observed and verify-uart accepted on the closure run: `display: bootstrap on`, `display: mode 1280x800 argb8888`, `windowd: present ok (seq=1 dmg=1)`, `display: first scanout ok`, `SELFTEST: display bootstrap guest ok`.
- Full closure gates now green in sequence: `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, `make clean`, `make build`, `make test`, `make run`.
- Follow-up ownership remains explicit and unchanged: `TASK-0055C` (visible SystemUI first frame), `TASK-0251` (display OS integration), `TASK-0056B` (input routing), and kernel/perf follow-ups.

## TASK-0055B critical delta report (resolved)

- **Bottom line**: closure-hardening concerns were validated and resolved by green reruns of all required gates.
- **Gate status**: complete and green for the required closure sequence.
- **AC1 / graphics-capable QEMU mode**: substantially satisfied. `visible-bootstrap` selects `NEXUS_DISPLAY_BOOTSTRAP=1`, `-display gtk`, and `-device ramfb`; existing headless/default runs are preserved. The profile distinction is correct: this is a harness/marker profile, not a SystemUI/launcher start profile.
- **AC2 / one surviving display authority**: improved by closure hardening. No second compositor or `fbdevd` substitute was introduced; `windowd` owns fixed-mode validation, present evidence, and deterministic bootstrap seed-surface pixels that `selftest-client` writes/tiled into the `ramfb` VMO. `selftest-client` still performs the low-level bootstrap write/configure step, so this remains bootstrap-display plumbing, not a full display daemon.
- **AC2 scope exception**: the original plan said not to absorb init/capability-distribution work; the user explicitly approved scope expansion. The implemented `device.mmio.fwcfg` grant is policy-gated, but currently granted to `selftest-client` as service policy rather than only when the visible bootstrap mode is active. This is not ambient MMIO, but it is broader than strict boot-mode least privilege.
- **AC3 / real visible first frame**: partially satisfied. The guest writes a full `1280x800` ARGB8888 VMO and QEMU `fw_cfg` DMA for `etc/ramfb` completes before markers are emitted. This proves guest-side RAMFB setup, not a captured visual artifact or readback from QEMU scanout. No screenshot/visual evidence artifact exists. If "visible" requires independent screenshot/manual artifact, this remains open; if QEMU `ramfb` DMA completion plus marker harness is accepted as the contract proof, it is covered.
- **AC3 / fake-proof marker honesty**: corrected in code/docs before retesting. The guest marker is now `SELFTEST: display bootstrap guest ok`, meaning guest-side mode/present/framebuffer-write/ramfb-config completed. `verify-uart` acceptance remains post-run harness evidence, not a guest-emitted fact.
- **AC4 / fail-closed rejects**: substantially satisfied for `windowd` visible contract surfaces. Tests cover invalid mode, stride, format, invalid display capability handoff, and pre-scanout marker attempts. Still missing if strict: host/unit coverage for malformed `fw_cfg` directory, absent `etc/ramfb`, DMA timeout/error, and capability grant denial. Those are currently exercised only indirectly by the QEMU path succeeding, not by negative tests.
- **AC5 / docs and handoff**: partially satisfied. Task/RFC/testing/ADR/status docs describe the chosen `ramfb` path and follow-up boundaries. The originally suggested `docs/display/simplefb_v1_0.md` was not created because the implementation is not simplefb/fbdevd. If docs require a display-bootstrap SSOT independent of RFC/task/testing docs, add a `ramfb` bootstrap doc instead of a misleading simplefb doc.
- **Tests as Soll-verification**: current host tests are meaningful for mode/capability/marker preconditions. The QEMU proof is meaningful for "boot a graphics-capable QEMU path and configure `ramfb`." The tests do not yet prove "real `windowd` output is what appears"; that belongs to `TASK-0055C` and must not be back-claimed here.
- **Future task expectations**:
  - `TASK-0055C` may assume QEMU can expose a fixed visible `ramfb` target and that `selftest-client` can configure it, but must still wire real `windowd`/SystemUI output to that target and replace the bootstrap pattern claim with visible `windowd` present proof.
  - `TASK-0251` still owns fuller display OS integration / `fbdevd`; no dirty-rect service, display settings, cursor, hotplug, or production display daemon claim exists here.
  - `TASK-0056B` owns deterministic visible cursor/hover/focus/click affordances; live input routing remains `TASK-0252`/`TASK-0253`.
  - `TASK-0055D` still owns rich dev display/start-profile presets; `visible-bootstrap` must not be reused as a SystemUI start profile.
  - `TASK-0288`/`TASK-0290` and related kernel lanes still own perf/latency and zero-copy/kernel production-grade closure.

## TASK-0055C implementation state

- `TASK-0055C` is `Done` with `RFC-0049` (`Done`).
- Implemented so far:
  - `source/services/systemui/` is split into small `profile`, `shell`, and `frame` modules,
  - SystemUI has minimal repo-owned TOML seeds for `desktop` profile + shell,
  - `windowd` visible-present evidence now composes the deterministic SystemUI first frame into the visible 1280x800 frame on host and exposes composed rows for OS/QEMU,
  - `selftest-client` writes `windowd`-composed rows to QEMU `ramfb`, not a raw SystemUI source buffer or selftest-owned sidecar composition,
  - `selftest-client`, proof-manifest, and `scripts/qemu-test.sh` use the 55C visible marker ladder.
- Green evidence so far:
  - `cargo test -p systemui -- --nocapture`,
  - `cargo test -p windowd -p ui_windowd_host -- --nocapture`,
  - OS-target `selftest-client` visible build check with `NEXUS_DISPLAY_BOOTSTRAP=1`,
  - `cargo test -p selftest-client -- --nocapture`,
  - `cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture`,
  - `cargo test -p ui_windowd_host reject -- --nocapture`,
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`,
  - `scripts/fmt-clippy-deny.sh`.
- Observed 55C QEMU ladder: `display: bootstrap on`, `display: mode 1280x800 argb8888`,
  `windowd: backend=visible`, `windowd: present visible ok`, `display: first scanout ok`,
  `systemui: first frame visible`, `SELFTEST: ui visible present ok`.
- Closure gates are green: `just test-all`, `just ci-network`, `make clean`, `make build`,
  `make test`, and `make run`.
- Scope boundary remains explicit: no input/cursor/focus, perf/smoothness, full display integration,
  dev-preset/start-profile matrix, GPU, or kernel/core production-grade closure claim.

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
