---
title: TASK-0054D UI v1a extension: kernel MM perf floor (VMO surface reuse + mapping discipline + cheap activate path)
status: Draft
owner: @kernel-mm-team @runtime @ui
created: 2026-03-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Address-space and mapping security floor: docs/rfcs/RFC-0004-safe-loader-guards.md
  - VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - UI present/input consumer baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

For early UI work, the most expensive memory-management problems are not “desktop-class VM” features like paging
policy or swap. The hot path is much narrower:

- VMO-backed surface buffers,
- image/media/doc payload mappings,
- repeated map/reuse cycles,
- and avoidable address-space activation churn around UI/process handoff.

This task defines a small **MM performance floor** for UI- and media-shaped workloads so that later compositor
tasks do not grow around a needlessly expensive mapping path.

## Goal

Deliver a kernel MM/UI floor focused on **surface and bulk-buffer reuse**, not a full VM redesign:

1. **VMO surface mapping discipline**:
   - define reuse-oriented mapping rules for surface/image/media buffers,
   - avoid repeated map/unmap churn when buffers can remain valid across frames.
2. **Cheap repeated-use path**:
   - optimize the common case where the same VMO/surface mapping is reused or re-presented,
   - avoid unnecessary work when mappings and permissions are unchanged.
3. **Address-space activation sanity**:
   - measure and reduce avoidable activate/switch churn on UI-shaped flows,
   - keep address-space ownership and W^X invariants intact.
4. **Explicit bounds for UI-sized buffers**:
   - alignment, size, and arena-use rules are documented and tested for surface/backbuffer consumers.

## Non-Goals

- Demand paging / swap / full VM redesign.
- Security model changes to W^X or guard pages.
- GPU driver memory management.
- A generalized “map anything anywhere” optimization path.

## Constraints / invariants (hard requirements)

- Preserve W^X and existing mapping security guarantees.
- No executable mappings for UI/media bulk buffers.
- Keep kernel MM ownership rules explicit and auditable.
- Optimize reuse and unchanged-state cases first; do not add speculative complexity.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

This task changes kernel MM hot paths and therefore is security-relevant.

### Threat model

- **Permission drift**: reuse optimization accidentally broadens mapping rights.
- **Arena confusion**: UI/media buffers map outside approved regions or exceed bounds.
- **Use-after-free / stale mapping**: reused VMO windows outlive validity assumptions.
- **Executable/data confusion**: performance shortcuts weakening W^X.

### Security invariants (MUST hold)

- W^X remains enforced for all user mappings.
- VMO size, offset, and arena bounds remain validated on every relevant mapping path.
- Reuse optimizations must not keep stale write access alive beyond the documented contract.
- Surface/media mappings remain non-executable and capability-gated.

### DON'T DO

- DON'T turn MM optimization into a bypass of existing mapping validation.
- DON'T assume “same VMO” means “same rights” without explicit checks.
- DON'T expand the user VMO arena or reuse rules without documented bounds.
- DON'T add UI-specific MM shortcuts that become a second mapping authority.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Host/kernel tests prove:
  - repeated mapping/reuse paths keep bounds and permissions correct,
  - unchanged-state cases avoid redundant work where intended,
  - surface-sized buffer alignment/size rules are deterministic.

### Proof (OS/QEMU) — gated

Deterministic markers / evidence:

- `SELFTEST: mm vmo reuse ok`
- `SELFTEST: mm ui mapping ok`
- `SELFTEST: mm activate churn ok`

Notes:

- Evidence can be emitted by selftest or userspace proofs, but success claims must be tied to real reuse behavior.

## Touched paths (allowlist)

- `source/kernel/neuron/src/mm/`
- `source/kernel/neuron/src/syscall/`
- `source/kernel/neuron/src/task/`
- `source/libs/nexus-abi/`
- `source/apps/selftest-client/`
- `docs/architecture/01-neuron-kernel.md`
- `docs/storage/vmo.md`
- `docs/testing/index.md`

## Plan (small PRs)

1. Document and implement reuse-oriented VMO/surface mapping rules.
2. Tighten unchanged-state/common-case MM paths before adding new complexity.
3. Add host/QEMU evidence for reuse, bounds, and activate-path sanity.
4. Sync docs so later UI tasks can rely on the MM floor explicitly.
