# Context Bundles (Low-Token)

<!--
CONTEXT
Small, stable context bundles to avoid expensive @codebase scans.
Use these in chat prompts to keep work deterministic and low-token.
-->

## Bundles (copy/paste)

### @core_context
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/stop_conditions.md`
- `.cursor/pre_flight.md`

### @task_context
- `tasks/TASK-XXXX-*.md`
- (linked) `docs/rfcs/RFC-XXXX-*.md`
- (linked) `docs/adr/XXXX-*.md`

### @task_0012_context
- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
- `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md`
- `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
- `docs/architecture/01-neuron-kernel.md`
- `docs/adr/0025-qemu-smoke-proof-gating.md`
- `docs/dev/platform/qemu-virtio-mmio-modern.md`
- `scripts/qemu-test.sh`

### @task_0012_touched
- `source/kernel/neuron/src/**`
- `scripts/run-qemu-rv64.sh` (only if needed for explicit SMP param wiring)
- `scripts/qemu-test.sh` (SMP marker gate wiring only)
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/README.md`
- `docs/testing/index.md` (only if marker-gating behavior changes)

### @touched
- Only the directories listed in the task's **Touched paths** allowlist.

### @quality_gates
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

## Standard instruction line (recommended)
Kontext strikt: @core_context @task_context @quality_gates @touched. Kein @codebase Scan.

## Standard instruction line (TASK-0012)
Kontext strikt: @core_context @task_0012_context @quality_gates @task_0012_touched. Kein @codebase Scan.
