# RFC-0053: Input v1.0b OS/QEMU live-input path (`hidrawd` + `touchd` + `inputd`)

- Status: In Progress
- Owners: @ui
- Created: 2026-05-04
- Last Updated: 2026-05-04
- Links:
  - Tasks: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`
    - `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + proof vectors)**: ⬜
- **Phase 1 (service wiring + reject floor)**: ⬜
- **Phase 2 (hardening + Gate-E closure sync)**: ⬜

Definition:

- "Complete" means the OS/QEMU live-input contract is implemented and the required host + OS proofs are green.
- "Complete" does not include latency/perf-budget closure; that remains `TASK-0056C`.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution sequencing and closure proof runs remain task-owned.

- **This RFC owns**:
  - deterministic OS/QEMU ingestion path from HID/touch sources to `inputd`,
  - routing contract from `inputd` to existing UI authorities (`windowd`/SystemUI/IME hooks),
  - fail-closed reject model and marker-honesty model for live-input closure.
- **This RFC does NOT own**:
  - host input-core algorithms already owned by RFC-0052 (`hid`, `touch`, `keymaps`, `repeat`, `pointer-accel`),
  - `windowd` hit-test/focus authority (owned by RFC-0050/0051),
  - IME/OSK full behavior and text stack breadth (`TASK-0146`/`TASK-0147`),
  - latency budget/perf closure (`TASK-0056C`).

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

## Non-Goals

- Kernel redesign or broad MMIO model changes.
- New authority for hit-test/focus/hover/click outside `windowd`.
- Full IME semantics, OSK behavior, or advanced text shaping.
- Latency-budget claims for smoothness/perf scenes (`TASK-0056C`).

## Constraints / invariants (hard requirements)

- **Single authority chain**: `hidrawd|touchd -> inputd -> windowd`; no sidecar routing authority.
- **Determinism**: equivalent source vectors produce equivalent `InputEvent` outcomes.
- **No fake success**: markers are emitted only after the associated state transition is asserted.
- **Bounded resources**: bounded subscriptions, bounded input queues, bounded retry loops, bounded marker payloads.
- **Fail-closed**: malformed frames/stale channels/invalid configs reject with stable classes.
- **No parser/keymap drift**: all parsing/keymap/repeat/accel behavior reuses RFC-0052 crates.

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
  - provides bounded subscriber stream API with explicit reject behavior for malformed payload and stale subscriber state.
- `touchd`:
  - ingests touch source data and emits normalized touch events from RFC-0052 `touch`,
  - supports deterministic synthetic mode for proof runs when real source is absent.
- `inputd`:
  - merges source streams into a bounded `InputEvent` stream,
  - applies keymap/repeat/accel through RFC-0052 crates,
  - routes to `windowd` and bounded IME hook integration without owning hit-test/focus.
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

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

### Deterministic markers (required, non-exhaustive)

- `hidrawd: ready`
- `hidrawd: device kbd`
- `hidrawd: device mouse`
- `touchd: ready`
- `inputd: ready`
- `inputd: keymap=de`
- `inputd: repeat start code=...`
- `inputd: dispatch windowd cursor=(x,y)`
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
- marker-before-state attempts reject (marker honesty).

Quality-gate closure before `Done`:

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

- [ ] **Phase 0**: contract + Soll/reject vectors frozen — proof: `task+RFC review`
- [ ] **Phase 1**: service wiring + reject floor green — proof: `TASK-0253 required host/os proofs`
- [ ] **Phase 2**: hardening + Gate-E sync green — proof: `quality gates + docs sync`
- [ ] Task linked with stop conditions + proof commands.
- [ ] Marker verification uses proof-manifest/harness ordering (no grep-only closure).
- [ ] Security-relevant negative tests exist (`test_reject_*`).
