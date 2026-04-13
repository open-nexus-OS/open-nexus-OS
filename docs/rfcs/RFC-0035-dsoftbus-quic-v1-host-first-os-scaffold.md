# RFC-0035: DSoftBus QUIC v1 host-first scaffold contract

- Status: In Progress
- Owners: @runtime
- Created: 2026-04-10
- Last Updated: 2026-04-10
- Links:
  - Execution SSOT: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - Program gate track: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - Status board: `tasks/STATUS-BOARD.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md`
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
    - `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md`

## Status at a Glance

- **Phase A (contract lock + fallback semantics)**: ✅
- **Phase B (host proof: QUIC + reject paths)**: ⬜
- **Phase C (OS-gated fallback markers)**: ⬜
- **Phase D (deterministic perf gate)**: ⬜
- **Phase E (closure sync + handoff evidence)**: ⬜

Definition:

- "Complete" means this QUIC v1 scaffold contract is implemented and all task-owned proof gates are green.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution truth remains in tasks.

- **This RFC owns**:
  - the transport-selection contract for `auto|tcp|quic` in `TASK-0021`,
  - strict-mode fail-closed behavior (`mode=quic`),
  - deterministic fallback-marker obligations while OS QUIC is disabled,
  - security invariants for downgrade resistance, ALPN/cert checks, and session authority preservation.
- **This RFC does NOT own**:
  - legacy `TASK-0001..0020` closure obligations already owned by `RFC-0034`,
  - mux v2 flow-control/keepalive semantics already owned by `RFC-0033`,
  - full reusable OS backend/core split work (`TASK-0022`) beyond boundary declarations.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` is authoritative for stop conditions and proof commands.
- If this RFC and task disagree on contract semantics, this RFC is authoritative and task text must be aligned.
- If this RFC and task disagree on progress/proof status, task is authoritative.

## Program alignment (RFC-0034 + production-gates track)

- This RFC intentionally follows the phase profile used in `RFC-0034` (A-E gates, no fake success, deterministic evidence).
- It keeps the DSoftBus group at `production-floor` trajectory as defined in `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
- It preserves status-board sequencing: `TASK-0021` is queue head after `TASK-0020` closure (`tasks/STATUS-BOARD.md`).

## Context

After `TASK-0020` closure, DSoftBus needs a host-first QUIC transport scaffold without destabilizing the TCP default path or creating fake OS support claims. Current OS backend state is still placeholder-only (`userspace/dsoftbus/src/os.rs` returns `Unsupported` paths), so OS QUIC must remain disabled by default in this slice.

## Goals

- Define a stable v1 selection contract for `auto|tcp|quic`.
- Enforce fail-closed strict QUIC mode with explicit downgrade behavior.
- Require deterministic fallback markers in OS/QEMU when QUIC is unavailable.
- Keep transport integration modular (`dsoftbusd` seams) and compatible with `TASK-0020` mux contract.

## Non-Goals

- Enabling full OS QUIC data path by default.
- Replacing or expanding `TASK-0022` reusable core/no_std backend scope.
- Reopening legacy `TASK-0001..0020` completion state.
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Determinism**: selection outcomes and markers are stable and bounded.
- **No fake success**: no `quic ok` or equivalent marker unless QUIC transport was truly selected and used.
- **No silent fallback**: `mode=quic` must fail closed on QUIC unavailability or validation failure.
- **Explicit fallback**: `mode=auto` fallback to TCP must emit deterministic audit markers.
- **Security floor**:
  - ALPN/cert mismatch rejects are deterministic,
  - transport handshake success does not bypass authenticated DSoftBus session authority,
  - negative `test_reject_*` proof paths are mandatory.
- **Stubs policy**: OS placeholder backend remains labeled and non-authoritative until `TASK-0022` closure.

## Proposed design

### Contract / interface (normative)

- Transport mode is explicitly selected as one of:
  - `tcp` (no QUIC attempt),
  - `quic` (strict mode; fail closed),
  - `auto` (attempt QUIC first, then deterministic TCP fallback).
- QUIC selection and fallback decisions must emit stable, grep-safe markers for harness verification.
- Mux semantics remain transport-agnostic and governed by `RFC-0033`; this RFC only defines transport selection and downgrade behavior.

### Marker contract (normative)

When OS QUIC is disabled and fallback path is exercised, markers must include:

- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`
- `dsoftbusd: transport selected tcp` (or one stable equivalent string)

No QUIC-success marker is allowed when TCP was selected.

### Phases / milestones (contract-level)

- **Phase A**: lock selection/fallback contract + security invariants.
- **Phase B**: host requirement suites for QUIC selection and reject paths are green.
- **Phase C**: canonical OS/QEMU fallback marker ladder is green.
- **Phase D**: deterministic perf envelope for selection path is defined and green.
- **Phase E**: docs/testing + board/order + handoff evidence synchronized.

## Security considerations

### Threat model

- downgrade forcing from QUIC to TCP without explicit signal,
- ALPN/certificate validation drift,
- confused-deputy risk where transport metadata is treated as app/session authority.

### Mitigations

- strict-mode fail-closed behavior (`mode=quic`),
- deterministic fallback marker audit in `mode=auto`,
- mandatory reject tests:
  - `test_reject_quic_wrong_alpn`
  - `test_reject_quic_invalid_or_untrusted_cert`
  - `test_reject_quic_strict_mode_downgrade`
  - `test_auto_mode_fallback_marker_emitted`
- preserve authenticated DSoftBus session gating independent of transport handshake.

### DON'T DO

- DON'T silently downgrade strict QUIC mode.
- DON'T emit QUIC success markers for fallback paths.
- DON'T treat ALPN/cert failures as warning-only behavior.
- DON'T merge `TASK-0022` extraction scope into `TASK-0021`.

## Failure model (normative)

- `mode=quic`:
  - QUIC unavailability, ALPN mismatch, or untrusted cert -> explicit hard failure.
- `mode=auto`:
  - QUIC failure -> deterministic fallback to TCP + mandatory fallback marker.
- Missing required markers/proofs means phase is not green.
- Distributed/performance claims without matching artifacts are invalid.

## Behavior-first proof selection (Rule 07)

### Target behavior (must be true at done)

- Transport selection is deterministic for `auto|tcp|quic` and does not destabilize default TCP bring-up.
- Strict QUIC mode rejects downgrade paths fail-closed.
- Auto mode fallback is explicit/auditable and does not pretend QUIC success.

### Main break point (dishonest/unsafe if broken)

- A regression that silently downgrades `mode=quic` to TCP (or treats ALPN/cert failure as warning-only) would make this task security-dishonest.

### Minimal proof shape (smallest honest set)

- **Primary proof (host integration + reject paths)**:
  - one positive transport-selection assertion (`quic` works when valid),
  - required negative tests:
    - `test_reject_quic_wrong_alpn`
    - `test_reject_quic_invalid_or_untrusted_cert`
    - `test_reject_quic_strict_mode_downgrade`
    - `test_auto_mode_fallback_marker_emitted`
- **Secondary proof (real boundary blind spot)**:
  - one canonical OS/QEMU fallback marker proof to confirm runtime wiring at stack boundary.
- **Only if distributed behavior is claimed**:
  - add `tools/os2vm.sh` proof; otherwise do not add 2-VM "for completeness."

### Anti-slop rules for this RFC

- Do not add unit+integration+QEMU+2VM+fuzz by default.
- Markers are supporting evidence, not the primary proof where host assertions are possible.
- If OS path remains placeholder/degraded, tests/markers must say so explicitly.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- quic --nocapture
```

Host suite must include the behavior-first minimum:

- one positive QUIC selection assertion,
- strict-mode downgrade reject,
- ALPN/cert reject paths,
- explicit fallback-marker assertion in auto mode.

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Proof (2-VM when distributed behavior is asserted)

```bash
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Regression / floor

```bash
cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp
```

### Deterministic markers (required in fallback path)

- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`
- `dsoftbusd: transport selected tcp` (or one stable equivalent)

Closure question for phase sign-off:

- Which test proves the intended behavior, and where would the break show up if it regressed?

## Alternatives considered

- Extend `RFC-0034` with QUIC contract: rejected (legacy closure RFC must remain bounded to `TASK-0001..0020`).
- Extend `RFC-0033` with transport-selection semantics: rejected (mux RFC scope would drift).
- Delay contract work until after `TASK-0022`: rejected (blocks host-first progress and weakens sequencing clarity).

## Open questions

- Should v1 host cert strategy remain ephemeral self-signed until device-identity integration follow-on, or require an earlier key-custody bridge? (owner: @runtime, follow-on bound to `TASK-0022+` planning)

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- scope remains limited to transport-selection/fallback contract,
- marker obligations stay deterministic and non-ambiguous,
- `TASK-0022` boundary remains explicit (no hidden scope absorption),
- phase status only flips when task-owned proofs are green.

---

## Implementation Checklist

**This section tracks implementation progress.**

- [x] **Phase A**: selection/fallback contract locked — proof: RFC + task alignment review.
- [ ] **Phase B**: host QUIC/reject suite green — proof: `cargo test -p dsoftbus -- quic --nocapture`
- [ ] **Phase C**: OS fallback markers green — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [ ] **Phase D**: deterministic perf gate green — proof: task-owned perf command(s) in `TASK-0021`
- [ ] **Phase E**: closure sync + handoff evidence complete — proof: task closeout docs + status sync
- [x] Task linked as execution SSOT (`tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`).
- [ ] Security negative tests exist and are green (`test_reject_*` for QUIC downgrade/validation).
