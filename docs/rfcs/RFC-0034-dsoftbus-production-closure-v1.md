# RFC-0034: DSoftBus production closure v1 (legacy TASK-0001..0020)

- Status: Done
- Owners: @runtime
- Created: 2026-04-10
- Last Updated: 2026-04-11
- Links:
  - Execution SSOT: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
    - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
    - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`

## Status at a Glance

- **Phase A (contract lock + gate profile)**: ✅
- **Phase B (legacy host proof closure)**: ✅
- **Phase C (legacy OS/2-VM marker closure)**: ✅
- **Phase D (legacy core+performance budget closure)**: ✅
- **Phase E (legacy hardening: fault/soak + release evidence)**: ✅

Definition:

- "Done" means the legacy production gate contract for `TASK-0001..0020` is implemented and task-owned proofs are green.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution truth remains in tasks.

- **This RFC owns**:
  - production-ready closure contract for legacy DSoftBus capabilities from `TASK-0001..0020`,
  - extraction of mandatory closure obligations into `TASK-0020` without rewriting historic `Done` statuses,
  - required evidence model for legacy closure (host + single-VM + 2-VM + perf + hardening).
- **This RFC does NOT own**:
  - detailed mux protocol semantics already owned by `RFC-0033`,
  - changing historical task status (`Done` remains `Done`),
  - defining contracts for subsequent DSoftBus tasks after the legacy closure scope.

### Relationship to tasks (single execution truth)

- `tasks/TASK-*.md` define stop conditions and proof commands.
- This RFC defines what "legacy DSoftBus 0001..0020 production-ready" means.
- If task and this RFC disagree on progress/proof, task is authoritative.
- If task and this RFC disagree on production gate contract, this RFC is authoritative and task text must be aligned.

### Execution sequencing rule (anti-preemption)

- Follow general task order strictly; execute one task at a time.
- While `TASK-0020` is `In Progress`, do not start proof-execution slices for subsequent DSoftBus tasks.
- Mandatory legacy closure obligations from this RFC are executed under `TASK-0020` first.

## Context

`TASK-0001..0019` established baseline DSoftBus functionality. `TASK-0020` is used as the closure slice to raise those legacy capabilities to a production-grade bar (determinism, boundedness, no fake success, hardening evidence), without reopening historical task scope.

## Goals

- Define a single production-gate profile (Core + Performance + Hardening) for legacy DSoftBus capability closure (`TASK-0001..0020`).
- Close extracted deltas from legacy tasks via `TASK-0020` proofs without changing old task status.
- Require deterministic, requirement-named proofs for host, OS single-VM, and 2-VM where distributed behavior is claimed.

## Non-Goals

- Rewriting old task history or re-opening completed tasks as "not done".
- Defining new contracts for DSoftBus tasks after `TASK-0020`.
- Introducing kernel changes for this closure contract.

## Constraints / invariants (hard requirements)

- **Determinism**: proof outcomes and marker ladders are deterministic and bounded.
- **No fake success**: no ready/ok marker without real behavior.
- **Bounded resources**: explicit limits for stream count, payload size, buffered bytes, credits/windows, retries.
- **Security floor**:
  - authenticated peer identity before sensitive operations,
  - deny-by-default behavior,
  - negative tests for bounds/state/auth failures.
- **Rust/API hygiene floor**:
  - typed domains (`newtype`) where class confusion is possible,
  - explicit ownership of mutable mux/session state,
  - `#[must_use]` on critical transition/accounting outcomes,
  - no unsafe `Send`/`Sync` shortcuts.

## Proposed design

### Legacy production gate profile (normative)

Legacy closure for `TASK-0001..0020` must satisfy:

1. **Contract gate**: explicit scope/invariants/reject labels in `TASK-0020` + linked RFCs.
2. **Host gate**: requirement-named tests (including `test_reject_*`) are green.
3. **OS gate (single-VM)**: canonical marker ladder is green with no synthetic success.
4. **Distributed gate (2-VM)**: canonical `tools/os2vm.sh` proof is green where distributed claims exist.
5. **Performance gate**: deterministic runtime budgets are green (`phase: perf`).
6. **Hardening gate**: bounded soak/fault checks and release evidence bundle are green (`phase: soak` + `release-evidence.json`).

### Phases / milestones (contract-level)

- **Phase A**: lock contract boundaries and closure profile.
- **Phase B**: host requirement-proof closure for extracted legacy obligations.
- **Phase C**: OS single-VM + 2-VM marker closure for extracted legacy obligations.
- **Phase D**: deterministic performance budgets green for extracted legacy obligations.
- **Phase E**: hardening (soak/fault) + release evidence bundle green.

### Legacy 0001..0020 Soll-test matrix (normative)

| Requirement family (0001..0020 origin) | Soll-proof (not implementation-detail proof) | Canonical evidence path |
|---|---|---|
| Authenticated session before payload (`TASK-0003B/0004/0005`) | reject unauthenticated/mismatched identity paths | host `test_reject_*` + handshake/identity markers |
| Discovery/admission boundedness (`TASK-0003C/0004`) | bounded reject behavior and deterministic paths | host requirement tests + canonical OS markers |
| Remote proxy authorization (`TASK-0005/0016/0017`) | deny-by-default service/cap checks | host reject suites + 2-VM marker evidence |
| Mux/backpressure/keepalive integrity (`TASK-0020`) | bounded flow-control rejects + keepalive teardown behavior | `mux_*` requirement suites + single-VM ladder + 2-VM ladder |
| Distributed correctness (`TASK-0005` lineage) | real 2-VM proof for distributed claims | `tools/os2vm.sh` summary artifacts |
| Marker honesty discipline (all tasks) | markers only with real state assertions | `scripts/qemu-test.sh` + `tools/os2vm.sh` contract checks |

### Marker honesty policy (normative)

- A marker counts as proof only when paired with real protocol/state assertions.
- Gated/stub capabilities stay explicitly marked until real behavior exists.
- "Soll" tests assert externally visible behavior/rejects, not implementation internals.

## Security considerations

### Threat model

- unauthorized session use,
- malformed or oversize protocol paths,
- credit/window abuse and starvation,
- fake-green marker paths that hide failures.

### Mitigations

- authenticated-only paths,
- fail-closed rejects for bounds/state/auth violations,
- deterministic bounded scheduling/flow-control/keepalive,
- mandatory negative tests and canonical harness proofs.

### DON'T DO

- DON'T mark old tasks as not-done.
- DON'T claim production readiness without green mandatory gates.
- DON'T add implicit fallback behavior.

## Failure model (normative)

- Missing mandatory gate evidence means closure is open.
- Security gate failures are fail-closed.
- Performance/hardening regressions block production closure.
- Distributed claims without 2-VM evidence are invalid.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Proof (2-VM distributed)

```bash
cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Performance / regression gates

```bash
cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp
```

### Hardening / release evidence

- `tools/os2vm.sh` `phase: soak` green (bounded rounds, zero fail/panic hits)
- `artifacts/os2vm/runs/<runId>/release-evidence.json` reviewed

## Alternatives considered

- Put all production closure into `RFC-0033`: rejected (would bloat mux RFC scope).
- Keep closure as ad-hoc notes only in task docs: rejected (no explicit contract).

## Open questions

- None.

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- each gate remains measurable with concrete commands/artifacts,
- legacy mapping stays drift-free (no historical status rewrites),
- scope remains limited to legacy `TASK-0001..0020` closure.

---

## Implementation Checklist

**This section tracks implementation progress.**

- [x] **Phase A**: Contract lock + gate profile defined.
- [x] **Phase B**: Legacy host proof closure is green.
- [x] **Phase C**: Legacy OS/2-VM marker closure is green.
- [x] **Phase D**: Legacy Core+Performance budgets are green.
- [x] **Phase E**: Legacy hardening (fault/soak + release evidence) is green.
- [x] Extracted legacy obligations are implemented under `TASK-0020` (no preemption by subsequent DSoftBus tasks).
- [x] Security-relevant negative tests for extracted legacy closure obligations are present and green.

Progress snapshot (2026-04-11):

- `TASK-0020` single-VM + 2-VM mux ladders are proven.
- `tools/os2vm.sh` deterministic budgets (`phase: perf`) are green.
- `tools/os2vm.sh` hardening soak (`phase: soak`) is green with rounds=2 and zero fail/panic hits.
- Release evidence bundle is emitted and reviewed:
  - `artifacts/os2vm/runs/os2vm_1775990226/release-evidence.json`
