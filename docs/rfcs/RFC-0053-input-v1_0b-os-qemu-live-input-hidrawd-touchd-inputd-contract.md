# RFC-0053: Input v1.0b OS/QEMU live-input path (`hidrawd` + `touchd` + `inputd`)

- Status: Done
- Owners: @ui
- Created: 2026-05-04
- Last Updated: 2026-05-11
- Links:
  - Tasks: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`
    - `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md`
    - `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + proof vectors)**: ✅
- **Phase 1 (service wiring + reject floor)**: ✅ host/service slice landed and green
- **Phase 2 (hardening + Gate-E closure sync)**: ✅ complete for this slice; host hardening, deterministic proof sync, interactive OS-start contracts, docs sync, and full broad gate closure are green

Definition:

- "Complete" means the OS/QEMU live-input contract is implemented and the required host + OS proofs are green.
- "Complete" does not include latency/perf-budget closure; that remains `TASK-0056C`.
- Current implementation reality:
  - `hidrawd`, `touchd`, and `inputd` now exist as host-verified service crates with deterministic reject suites,
  - bounded `ime` / `systemui` hook seams, canonical `settingsd` input keys, service IDL seeds, and expanded `nx input` / `nx postflight input` surfaces exist,
  - the RFC-0052 carry-in crates now compile for the OS target at library level,
  - `inputd` now routes touch through `windowd` instead of only recording normalized touch dispatches,
  - kernel/runtime service-scale work now bounds the focused proof lane: page-table/address-space/VMO pressure diagnostics landed, per-service kernel mapping cost was reduced, and `exec_v2` rollback cleanup now reclaims new address spaces,
  - minimal OS service payload entries and startup markers now exist for `hidrawd`, `touchd`, and `inputd` in the default init-lite/QEMU service set,
  - the focused startup proof is green under `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`,
  - the deterministic `visible-bootstrap` scene is green under `verify-uart` and now emits full-window/cursor/hover/click/keyboard markers backed by framebuffer assertions,
  - the proof-manifest UART verifier now tolerates non-UTF8 UART noise, and `selftest-client` uses a targeted 512 KiB heap opt-in for the heavy visible scene proof,
  - `make run` / `just start` now have explicit interactive runtime-mode contracts (`interactive-minimal` and `interactive-full`) and must not emit fake deterministic proof markers,
  - focused contracts now cover the previously observed KPGF class (`neuron-boot.map` private selftest stack retention), late `fw_cfg` mode/profile delivery, and VMO arena headroom for the live ramfb framebuffer,
  - the former live-lane blocker is now closed through the `RFC-0054` driver-owner polling path instead of a `selftest-client` bridge,
  - focused proofs rerun green on the current tree: `cargo test -p virtio-input -- --nocapture`, `cargo test -p hidrawd -- --nocapture`, `cargo test -p inputd -- --nocapture`, `cargo test -p fbdevd -- --nocapture`, `cargo test -p selftest-client --test boot_cfg_runtime -- --nocapture`, `cargo test -p nx --test interactive_os_startup -- --nocapture`, `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s scripts/qemu-test.sh --profile=visible-bootstrap`, and `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os visible-bootstrap`,
  - broad closure gates were rerun explicitly: `just dep-gate`, `just diag-os`, `just diag-host`, `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, `make clean -> make build`, `make test`, `RUN_TIMEOUT=220s make run`, and `RUN_TIMEOUT=220s just start` are green.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution sequencing and closure proof runs remain task-owned.

- **This RFC owns**:
  - deterministic OS/QEMU ingestion path from HID/touch sources to `inputd`,
  - routing contract from `inputd` to existing UI authorities (`windowd`/SystemUI/IME hooks),
  - fail-closed reject model and marker-honesty model for live-input closure,
  - narrow kernel/runtime service-scale prerequisites needed to run the 0253 services as real init-lite service processes.
- **This RFC does NOT own**:
  - host input-core algorithms already owned by RFC-0052 (`hid`, `touch`, `keymaps`, `repeat`, `pointer-accel`),
  - the minimal `virtio-input` driver-layer contract and long-term `device.mmio.input` ownership rules now owned by RFC-0054,
  - `windowd` hit-test/focus authority (owned by RFC-0050/0051),
  - IME/OSK full behavior and text stack breadth (`TASK-0146`/`TASK-0147`),
  - latency budget/perf closure (`TASK-0056C`),
  - broad kernel redesign beyond the service-scale fixes required for this proof.

### Relationship to tasks (single execution truth)

- `TASK-0253` is the execution SSOT for this contract seed.
- `TASK-0253` must consume RFC-0052 authority crates and must not fork parser/keymap/repeat/accel logic.

## Context

`TASK-0252` closed the host-first input core. The next honest step is OS/QEMU live-input ingestion with explicit authority boundaries and non-fake closure evidence. Without a contract seed here, service-local drift (duplicate tables/parsers, marker-only closure, parallel routing authorities) is likely.

## Goals

- Define one deterministic, bounded, and fail-closed live-input service chain:
  - `hidrawd` and `touchd` source ingestion,
  - `inputd` normalization/merge/dispatch,
  - `windowd`/SystemUI/IME hook integration without authority drift.
- Define proof obligations that combine behavior assertions and marker verification (no grep-only closure).
- Keep implementation modular and maintainable with explicit Rust API/ownership discipline.
- Define the runtime prerequisites for running the input stack as real OS services without kernel heap exhaustion.

## Non-Goals

- Broad kernel redesign or broad MMIO model changes.
- New authority for hit-test/focus/hover/click outside `windowd`.
- Full IME semantics, OSK behavior, or advanced text shaping.
- Latency-budget claims for smoothness/perf scenes (`TASK-0056C`).

## Constraints / invariants (hard requirements)

- **Single authority chain**: `hidrawd|touchd -> inputd -> windowd`; no sidecar routing authority.
- **Determinism**: equivalent source vectors produce equivalent `InputEvent` outcomes.
- **No fake success**: markers are emitted only after the associated state transition is asserted.
- **Bounded resources**: bounded subscriptions, bounded input queues, bounded retry loops, bounded marker payloads.
- **Kernel/runtime scalability**: the normal QEMU service set plus `hidrawd`/`touchd`/`inputd` must not rely on a marker-only or script-gated workaround for kernel heap exhaustion.
- **Fail-closed**: malformed frames/stale channels/invalid configs reject with stable classes.
- **No parser/keymap drift**: all parsing/keymap/repeat/accel behavior reuses RFC-0052 crates.
- **Truth before distribution**: raw receive truth must become visible and testable before `inputd` routing or proof-scene observation is used to diagnose failures.

Rust quality floor (mandatory):

- use domain newtypes where raw primitives are ambiguity-prone (event IDs, device IDs, queue indices, config ranges),
- make ownership transfer explicit at service boundaries,
- apply `#[must_use]` for decision-bearing outcomes where ignored results would hide failures,
- no unsafe blanket `Send`/`Sync` shortcuts; rely on compiler-checked ownership and explicit synchronization,
- avoid monoliths: keep service crates split into focused modules (ingest, normalize, route, config, marker/test seams).

## Proposed design

### Contract / interface (normative)

- `hidrawd`:
  - ingests keyboard/mouse source data and emits typed events from RFC-0052 `hid`,
  - owns the explicit receive/adapter truth seam for raw input delivered by the driver layer from RFC-0054,
  - provides bounded subscriber stream API with explicit reject behavior for malformed payload and stale subscriber state.
- `touchd`:
  - ingests touch source data and emits normalized touch events from RFC-0052 `touch`,
  - supports deterministic synthetic mode for proof runs when real source is absent.
- `inputd`:
  - merges source streams into a bounded `InputEvent` stream,
  - applies keymap/repeat/accel through RFC-0052 crates,
  - routes to `windowd` and bounded IME hook integration without owning hit-test/focus.
- observer/debug posture:
  - `selftest-client` remains a downstream observer of distribution/visible truth,
  - it must not become the authority for what arrived from the input device.
- `windowd`/SystemUI/IME hook seam:
  - consumes routed events and emits visible-input markers only after routed-state assertions.

Suggested module shape (non-normative but recommended):

- `source/services/hidrawd/src/{main,service,ingest,error,types}.rs`
- `source/services/touchd/src/{main,service,ingest,error,types}.rs`
- `source/services/inputd/src/{main,service,merge,route,config,error,types}.rs`

### Phases / milestones (contract-level)

- **Phase 0**: freeze marker/order contract and Soll + reject vectors.
- **Phase 1**: implement source services + `inputd` routing with reject floor.
- **Phase 2**: harden settings/CLI/postflight seams and synchronize Gate-E quality evidence.
- **Phase 3**: fix kernel/runtime service-scale blockers so the three input services can run as real init-lite service processes in the normal proof path.
- **Phase 4**: replace marker-only visual assertions with a diagnosable visible-bootstrap scene: full colored window, mouse-following pixel, hover/click square, and keyboard-input square.
- **Phase 5**: land the minimal `virtio-input` driver layer and service ownership rules from RFC-0054 so QEMU live input is not bridged through `selftest-client`.
- **Phase 6**: close the host-driven interactive OS-start lane: `make run` and `just start` must launch the same live QEMU path with honest breadcrumb levels and real mouse/keyboard reaction, after receive truth is already proven upstream of UI observation.

## Security considerations

- **Threat model**:
  - malformed or adversarial HID/touch input,
  - stale/forged subscriber or routing state,
  - accidental authority drift between `inputd` and `windowd`,
  - marker-only false success.
- **Mitigations**:
  - fail-closed parsing/normalization via RFC-0052 crates,
  - stable reject classes and bounded queues,
  - explicit authority split (`inputd` route, `windowd` decide),
  - marker verification coupled to behavior assertions and proof-manifest order checks.
- **DON'T DO list**:
  - do not duplicate keymap tables or parser logic inside services,
  - do not emit `... ok` markers from unverified branches,
  - do not claim perf-budget closure in this RFC/task slice.

## Failure model (normative)

- malformed HID/touch payload -> deterministic reject (no synthesized success event),
- invalid keymap/repeat/accel config -> deterministic reject (no silent clamp-to-success),
- stale/unauthorized route target -> deterministic reject with bounded retry behavior,
- absent source device in required profile -> explicit failure marker path (no hidden fallback claim),
- no silent fallback for authority decisions or marker emission.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p input_v1_0_host -- --nocapture
```

Carry-in host floor from RFC-0052 remains required as algorithmic authority baseline.
Task-local host proofs must then extend that floor with:

```bash
cd /home/jenning/open-nexus-OS && cargo test -p hidrawd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p touchd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p inputd -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

This proof now runs green with the normal QEMU service set plus real
`hidrawd`, `touchd`, and `inputd` init-lite service processes in the focused
proof lane. The host-driven interactive start lane is additional closure scope;
it must not replace or weaken this deterministic harness proof.

The final visible proof must be visual and diagnosable:

- full colored window background,
- real mouse movement must produce one visible pixel that follows routed pointer motion,
- hover/click rectangle reaction must remain visible in the proof scene,
- keyboard rectangle reaction must remain visible in the proof scene,
- one pixel follows routed pointer motion in the proof scene,
- bottom-left square changes color on hover and click,
- right-side square changes color on keyboard input,
- UI-side logs/errors identify observed state transitions and failure causes.

### Proof (Interactive OS start)

The interactive lane is not a deterministic acceptance harness and must not emit
fake `SELFTEST:` success markers. It is a live OS bring-up path with honest
breadcrumbs:

```bash
cd /home/jenning/open-nexus-OS && make build && make run
cd /home/jenning/open-nexus-OS && just start
```

Required behavior before `TASK-0253` can close:

- `make run` reuses the latest `make build` artifacts and starts QEMU live in
  `interactive-minimal` mode,
- `just start` performs its own build and starts the same live runner in
  `interactive-full` mode,
- Final host-driven live QEMU proof must show real mouse movement, hover/click rectangle reaction, and keyboard rectangle reaction in the same scene,
- the live lane is backed by the real RFC-0054 `virtio-input` driver path rather
  than a permanent `selftest-client` bridge,
- the scene presents a full colored window, a visible mouse-following pixel, a
  hover/click rectangle, and a keyboard-input rectangle,
- failures emit stable labels such as `bootstrap: failed fw-cfg-map`,
  `bootstrap: failed fw-cfg-signature`,
  `bootstrap: failed ramfb-file-missing`,
  `bootstrap: failed framebuffer-vmo`, or
  `bootstrap: failed interactive-scene-evidence`.

### Deterministic markers (required, non-exhaustive)

- `hidrawd: ready`
- `hidrawd: device kbd`
- `hidrawd: device mouse`
- `touchd: ready`
- `inputd: ready`
- `hidrawd: os service payload ready`
- `touchd: os service payload ready`
- `inputd: os service payload ready`
- `inputd: keymap=de`
- `inputd: repeat start code=4`
- `inputd: dispatch windowd cursor=(36,28)`
- `systemui: imed show`
- `systemui: imed hide`
- `SELFTEST: input keymap de ok`
- `SELFTEST: input cursor ok`
- `SELFTEST: input touch ok`
- `SELFTEST: input repeat ok`

Reject-proof requirements (must exist as `test_reject_*` class proofs):

- malformed HID frame rejects,
- malformed touch sample/sequence rejects,
- stale/invalid subscriber or route target rejects,
- invalid keymap/repeat/accel config rejects,
- bounded queue overflow rejects,
- marker-before-state attempts reject (marker honesty).

Quality-gate closure before `Done`:

- `just dep-gate`
- `just diag-os`
- `just diag-host`
- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean` -> `make build` -> `make test` -> `make run`

Perf boundary honesty:

- 0253 proves bounded/measurable live-input behavior only,
- perf-budget closure is explicit follow-up (`TASK-0056C`).

## Alternatives considered

- Build live-input behavior directly inside `windowd` (rejected: authority blur and maintenance drift).
- Service-local reimplementation of keymap/repeat/accel logic (rejected: duplicates RFC-0052 authority and weakens determinism).

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: contract + Soll/reject vectors frozen — proof: `task+RFC review`
- [x] **Phase 1**: service wiring + reject floor green — proof: `TASK-0253 required host/os proofs`
- [x] **Phase 2**: hardening + Gate-E sync green — proof: `quality gates + docs sync`
- [x] Task linked with stop conditions + proof commands.
- [x] Host `hidrawd` / `touchd` / `inputd` packages and reject suites are green.
- [x] Host hardening surfaces exist for `ime`, `systemui`, `settingsd`, `nx input`, and `nx postflight input`.
- [x] Marker verification uses proof-manifest/harness ordering (no grep-only closure).
- [x] Security-relevant negative tests exist (`test_reject_*`).
- [x] RFC-0052 carry-in crates are OS-target compatible so the real live-input path can link into `selftest-client`.
- [x] Kernel/runtime service-scale slice and focused startup proof are green.
- [x] Deterministic visible scene proof emits pixel/state-backed markers under `verify-uart`.
- [x] Interactive OS-start contracts distinguish `make run` minimal breadcrumbs from `just start` full breadcrumbs.
- [x] Focused boot/resource contracts cover private selftest stack linker retention, late `fw_cfg` runtime config, and VMO arena framebuffer headroom.
- [x] RFC-0054 driver-layer slice is implemented so live QEMU input no longer depends on a permanent `selftest-client` bridge.
- [x] Final host-driven live QEMU proof shows real mouse hover/click and keyboard rectangle reaction.
- [x] Broad closure gates rerun explicitly for closeout, including `scripts/fmt-clippy-deny.sh`, `just test-all`, `just ci-network`, and `make clean` -> `make build` -> `make test` -> `make run`.
