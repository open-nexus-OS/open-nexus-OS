# Current Handoff: TASK-0014 Observability v2 (metrics/tracing) — PREP READY

**Date**: 2026-02-11  
**Status**: TASK-0013 is closed/archived. TASK-0014 is prepared with follow-up wiring, security section, and drift corrections.

---

## Baseline fixed (carry-in)

- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`: Done.
- `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`: Implemented (v1).
- Archived snapshot created:
  - `.cursor/handoff/archive/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- Upstream platform floors unchanged:
  - `TASK-0006` logd v1 available as bounded sink.
  - `TASK-0009` `/state` substrate available for bounded WAL/retention slices.

## Active focus (TASK-0014)

- Active task: `tasks/TASK-0014-observability-v2-metrics-tracing.md`.
- Seed contract: `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md`.
- Task header synced:
  - `enables` + `follow-up-tasks` added (`TASK-0038`, `TASK-0040`, `TASK-0041`, `TASK-0143`, `TASK-0046`).
  - explicit prerequisites now include `TASK-0006`, `TASK-0009`, `RFC-0019`, and `TASK-0013` producer baseline.
- Security coverage added in task:
  - threat model + invariants + DON'T DO + mitigations + security proof requirements.
  - deterministic reject markers and `test_reject_*` style negative tests mandated.
- Scope guard fixed to avoid drift:
  - local metrics/spans + logd export only in TASK-0014,
  - remote/cross-node stays in follow-up tasks (`TASK-0040`/`TASK-0038`).

## Next implementation slice (start here)

- Implement `source/services/metricsd/` and `userspace/nexus-metrics/` with bounded series/span state.
- Keep kernel untouched; all work remains userspace and OS-lite compatible.
- Prove via deterministic host tests + QEMU marker ladder against logd export path.

## Guardrails

- No fake success markers (`ready`/`ok` only after real behavior).
- No unbounded cardinality, payload, or live-span growth.
- Policy/identity decisions based on kernel-authenticated `sender_service_id`, never payload claims.
- Preserve modern virtio-mmio deterministic proof floor and bounded test timeouts.
