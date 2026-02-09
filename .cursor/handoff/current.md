# Current Handoff: TASK-0011 Kernel Simplification (RFC-0001) — Phase A (text-only) + Phase B (physical reorg)

**Date**: 2026-02-09  
**Goal**: Land TASK-0011 as a **logic-preserving** simplification pass with **zero behavior / marker changes**:

- Phase A: text-only (headers + docs)
- Phase B: physical reorg (moves + wiring only)

---

## Execution truth (anti-drift)

- **Task is the execution SSOT**: `tasks/TASK-0011-kernel-simplification-phase-a.md`
- **RFC is the contract seed / rationale**: `docs/rfcs/RFC-0001-kernel-simplification.md`
- **Touched paths allowlist** (task-owned):
  - `source/kernel/neuron/src/**`
  - `docs/**`

## Proof gates (must stay green; no marker list changes)

- **Canonical QEMU marker contract** (must remain identical):

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

## Scope guardrails

- **Phase A allowed**: module headers, doc cross-links, diagnostic index documentation, TEST_SCOPE/TEST_SCENARIOS documentation.
- **Phase A forbidden**: any runtime behavior changes, ABI/marker changes, code refactors that could alter execution.

- **Phase B allowed**: file moves/renames and module wiring updates to match the target tree in TASK-0011, plus docs path updates.
- **Phase B forbidden**: any semantic changes (including refactors), ABI/marker changes, dependency changes.

## Notes

- Keep PRs small and mechanical. If a change risks runtime behavior, it is out of scope for TASK-0011.

## Archive pointer

- Previous handoff snapshot (TASK-0009 / RFC-0018/0019): `.cursor/handoff/archive/TASK-0009-persistence-v1-virtio-blk-statefs.md`
