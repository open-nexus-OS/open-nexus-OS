# RFC-0037: DSoftBus QUIC v2 OS enablement gated contract

- Status: Done
- Owners: @runtime
- Created: 2026-04-15
- Last Updated: 2026-04-15
- Links:
  - Execution SSOT: `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
  - Follow-up implementation task: `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
  - Program gate track: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - Status board: `tasks/STATUS-BOARD.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md`
    - `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
    - `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`

## Status at a Glance

- **Phase A (contract lock + enablement semantics)**: ✅
- **Phase B (host proof: fail-closed integrity)**: ✅
- **Phase C (OS QUIC session proof in QEMU)**: ✅
- **Phase D (deterministic feasibility/security budgets)**: ✅
- **Phase E (closure sync + evidence refresh)**: ✅

Definition:

- `Done` means OS QUIC session behavior is implemented and proven within the current production-floor scope.
- This RFC no longer represents a blocked/no-go gate state.

## Scope boundaries (anti-drift)

This RFC owns the `TASK-0023` contract for OS QUIC-v2 session enablement.

- **This RFC owns**:
  - OS QUIC-v2 session marker contract and fail-closed behavior,
  - no_std-friendly datagram framing + Noise-XK authentication path,
  - deterministic/bounded reject rules for malformed or unsafe inputs.
- **This RFC does NOT own**:
  - full IETF QUIC/TLS parity in OS path,
  - advanced congestion tuning/perf breadth (`TASK-0044`),
  - kernel/MMIO contract changes.

### Relationship to tasks

- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md` remains execution SSOT.
- RFC is normative for contract semantics; task is normative for progress/proof status.

## Program alignment

- Aligns `DSoftBus & Distributed` to `production-floor` with real OS QUIC session evidence.
- Marker honesty remains strict: success markers are emitted only after real handshake/session checks.

## Context

`TASK-0021` delivered host-first QUIC scaffolding.
`TASK-0022` delivered no_std core seams.
`TASK-0023` now closes OS-side session enablement with real QEMU evidence.

## Goals

- Real OS QUIC-v2 session behavior over UDP facade with Noise-XK identity binding.
- Preserve strict fail-closed host reject behavior and feasibility guard contracts.
- Keep mux-v2 behavior unchanged across transport boundary.

## Non-Goals

- Full IETF QUIC stack parity in OS path.
- 0-RTT and advanced congestion tuning in this slice.
- Kernel-side contract changes.

## Constraints / invariants (hard requirements)

- Deterministic/bounded retries, parsing, and marker order.
- No silent fallback in QUIC-required profile.
- Untrusted frames are bounded and rejected deterministically.
- Security failures (identity/auth) are hard rejects.
- Rust discipline: `#[must_use]`, explicit ownership, reviewed `Send`/`Sync`.

## Proposed design

### Contract / interface (normative)

- Host transport selection remains `DSOFTBUS_TRANSPORT=tcp|quic|auto`.
- OS session path in `TASK-0023` uses QUIC-v2 datagram framing over UDP facade with Noise-XK auth.
- QEMU QUIC-required profile MUST show QUIC success markers and MUST reject fallback markers.

### Marker contract (normative)

Required in QUIC-required OS profile:

- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

Forbidden in QUIC-required OS profile:

- `dsoftbusd: transport selected tcp`
- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

## Security considerations

### Threat model

- silent downgrade from QUIC-required profile,
- identity/auth acceptance drift,
- malformed/oversized frame abuse.

### Mitigations

- strict marker gate in `scripts/qemu-test.sh`,
- deterministic reject-path contracts (`test_reject_*`),
- bounded frame parsing and explicit nonce correlation.

### DON'T DO

- DON'T emit fallback markers in QUIC-required profile.
- DON'T convert auth failures into warnings.
- DON'T bypass policy checks due to transport mode.

## Behavior-first proof selection

### Target behavior

- OS QUIC session path is real and marker-proven in QEMU.
- Host fail-closed behavior remains green.
- Feasibility/reject budget contracts remain green and explicit.

### Main break point

- Any regression that reintroduces silent fallback or marker-only success claims.

### Minimal proof shape

- Host contract proofs:
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
- OS/service proofs:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- Hygiene:
  - `just dep-gate && just diag-os`

## Baseline evidence refresh (2026-04-16)

- Host floors green:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS marker floor green:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - observed markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - fallback markers absent in QUIC-required profile.

## Open questions

- Should the OS path converge toward full IETF QUIC stack parity, or stay on the current no_std-friendly QUIC-v2 framing profile?
- Which additional tuning/perf targets should be carried by `TASK-0044`?

---

## Implementation Checklist

- [x] **Phase A**: enablement semantics locked in task/RFC.
- [x] **Phase B**: host fail-closed suites green.
- [x] **Phase C**: OS QUIC session markers proven in QEMU.
- [x] **Phase D**: deterministic feasibility/reject budgets green.
- [x] **Phase E**: closure sync and evidence refresh complete.
