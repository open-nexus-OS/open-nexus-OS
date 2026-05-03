# RFC-0052: Input v1.0a host-first core (HID/touch + keymaps + repeat + pointer acceleration)

- Status: In Progress
- Owners: @ui
- Created: 2026-05-03
- Last Updated: 2026-05-03
- Links:
  - Tasks: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`

## Status at a Glance

- **Phase 0 (contract + Soll-test vectors freeze)**: 🟨
- **Phase 1 (host core implementation + reject proofs)**: ⬜
- **Phase 2 (hardening + 0253 integration readiness)**: ⬜

Definition:

- "Complete" means the contract is defined and the required host proof gates are green. Live OS/QEMU input closure is explicitly owned by `TASK-0253`.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - deterministic host contracts for HID parsing, touch normalization, base keymaps, key repeat, and pointer acceleration,
  - security and boundedness invariants for untrusted input parsing,
  - test-first proof posture (Soll behavior + reject paths before closure claims),
  - implementation quality floor for Rust ownership/newtypes/`must_use` and maintainable crate/module shape.
- **This RFC does NOT own**:
  - OS/QEMU device ingestion, DTB/device wiring, `hidrawd`/`touchd`/`inputd` daemons, or `nx input` CLI (`TASK-0253`),
  - full IME/OSK behavior (`TASK-0146`/`TASK-0147`),
  - kernel/MMIO redesign.

### Relationship to tasks (single execution truth)

- `TASK-0252` is the execution SSOT for this contract seed (stop conditions + proof commands).
- `TASK-0253` consumes this contract and must not fork parsing/keymap/repeat/accel authority.

## Context

After deterministic visible-input closure (`TASK-0056B`), the next honest step is a reusable host-first input core. Without a shared contract here, `inputd`/IME/device integration tends to drift into duplicated parsers/tables and fake-green marker stories.

## Goals

- Define stable, deterministic host-side contracts for HID, touch, keymap, repeat, and pointer acceleration.
- Lock one keymap authority surface that later IME work extends instead of duplicating.
- Enforce proof-first behavior verification with explicit reject paths for malformed/untrusted input.
- Keep implementation split across focused crates/modules (no monolithic 2k LOC entrypoints).

## Non-Goals

- No live pointer/keyboard QEMU proof in this RFC.
- No service-level routing authority changes (`windowd` remains hit-test/focus authority).
- No standalone marker contract for 0252 success.

## Constraints / invariants (hard requirements)

- **Determinism**: same input vectors produce byte-identical logical output sequences.
- **No fake success**: no `ready/ok` marker contract for 0252 closure; host assertions are authoritative.
- **Bounded resources**: bounded lookup tables, bounded repeat state, bounded acceleration outputs.
- **Single authority**: one shared keymap contract consumed by `inputd` and future `imed`.
- **Rust quality floor**:
  - domain newtypes for key codes/layout IDs/ranges where ambiguity risks drift,
  - explicit ownership and borrow boundaries; avoid hidden shared mutable state,
  - no unsafe `Send`/`Sync` impls; rely on compiler derivation where possible,
  - apply `#[must_use]` on result/value types where ignoring return values would hide errors.
- **Transport-neutral now, virtio-mmio-ready later**: host core APIs stay transport-agnostic so `TASK-0253` can integrate modern virtio-mmio-backed device paths without parser contract churn.

## Proposed design

### Contract / interface (normative)

- `hid` crate:
  - parse USB-HID boot protocol keyboard/mouse reports into deterministic event records,
  - malformed/incomplete reports fail closed with stable reject classes.
- `touch` crate:
  - normalize touch source samples into ordered `down/move/up` events with deterministic coordinate handling.
- `keymaps` crate:
  - table-driven mappings for at least `us` and `de` in this phase, with extensible layout table contract for `jp/ko/zh`,
  - deterministic modifier handling (including AltGr path for DE),
  - no locale/env probing.
- `key-repeat` crate:
  - deterministic repeat scheduler over injectable time source,
  - explicit range validation for delay/rate settings.
- `pointer-accel` crate:
  - monotonic linear acceleration curve with explicit bounds and deterministic arithmetic.

Module/layout contract:

- crates split by authority (`hid`, `touch`, `keymaps`, `key-repeat`, `pointer-accel`);
- each crate keeps small public API and focused internal modules;
- avoid monolithic crate-level `main.rs`/`lib.rs` growth by extracting parser/model/error/test-vector modules.

### Phases / milestones (contract-level)

- **Phase 0**: freeze contract signatures + Soll test vectors + reject vectors.
- **Phase 1**: implement host core crates to satisfy deterministic behavior/reject proofs.
- **Phase 2**: hardening for integration readiness (API polish, docs, authority boundaries explicit for `TASK-0253`).

## Security considerations

- **Threat model**:
  - malformed or adversarial HID/touch input frames,
  - keymap drift causing inconsistent shortcut/text semantics,
  - timing-based nondeterminism in repeat behavior,
  - hidden fallback behavior that silently accepts invalid config.
- **Mitigations**:
  - fail-closed parsers with stable reject classes,
  - table-driven keymaps and explicit config validation,
  - injectable monotonic time source in tests,
  - bounded math and explicit overflow/bound checks,
  - no marker-only closure claims.
- **DON'T DO list**:
  - do not use host locale/environment as keymap authority,
  - do not implement duplicate keymap/parsing logic in downstream services,
  - do not add unsafe `Send`/`Sync` or global mutable parser state,
  - do not emit success markers for behavior that is not asserted by tests.
- **Open risks**:
  - initial layout coverage beyond `us`/`de` may remain partial; gaps must be explicit and fail closed.

## Failure model (normative)

- Malformed HID/touch frame: deterministic reject error, no synthesized success event.
- Invalid repeat settings (delay/rate out of allowed range): deterministic reject, no implicit clamping-to-success.
- Invalid pointer accel config: deterministic reject, no silent fallback curve.
- Unknown keymap ID: deterministic reject; fallback requires explicit caller choice and test proof.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p input_v1_0_host -- --nocapture
```

Required proof classes:

- keymap Soll mappings (EN/DE, umlauts/symbols/modifiers),
- repeat timing determinism with simulated time source,
- acceleration monotonic + bounded behavior,
- touch sequence ordering (`down -> move* -> up`),
- `test_reject_*` for malformed HID/touch frames and invalid repeat/accel config.

### Proof (OS/QEMU)

```bash
# not part of RFC-0052 closure; owned by TASK-0253
```

### Deterministic markers (if applicable)

- None for RFC-0052 closure. Marker-based live-input proofs belong to `TASK-0253`.

## Alternatives considered

- Build directly in `inputd` service first (rejected: duplicates parser/mapper logic and harms host-first determinism).
- Keep one combined input-core crate (rejected: higher drift/ownership risk and harder long-term maintenance).

## Open questions

- Should Phase 1 require full `jp/ko/zh` behavior proofs or only contract scaffolding + fail-closed placeholders? (Owner: @ui; decide before Phase 1 completion.)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: contract signatures + Soll/reject test vectors frozen — proof: `review task+RFC contract alignment`
- [ ] **Phase 1**: host core behavior + reject proofs green — proof: `cargo test -p input_v1_0_host -- --nocapture`
- [ ] **Phase 2**: hardening + integration-readiness docs synced — proof: `docs/task sync review`
- [ ] Task linked with stop conditions + proof commands.
- [ ] Marker contract explicitly N/A for 0252 and delegated to `TASK-0253`.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
