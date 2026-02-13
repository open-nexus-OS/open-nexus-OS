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

### @task_0012b_context
- `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`
- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
- `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md`
- `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md`
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/16-rust-concurrency-model.md`
- `scripts/qemu-test.sh`

### @task_0012b_touched
- `source/kernel/neuron/src/core/smp.rs`
- `source/kernel/neuron/src/core/trap.rs`
- `source/kernel/neuron/src/sched/mod.rs`
- `source/kernel/neuron/src/types.rs` (only if CPU/Hart ID helpers need narrow updates)
- `scripts/qemu-test.sh` (only if marker ordering/gating needs explicit sync)
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/16-rust-concurrency-model.md`
- `docs/testing/index.md` (only if proof/marker contract changes)

### @task_0013_context
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`
- `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`
- `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
- `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md`
- `tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md`
- `tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md`
- `scripts/qemu-test.sh`

### @task_0013_touched
- `source/kernel/neuron/src/syscall/{mod.rs,api.rs}` (QoS syscall surface when required)
- `source/kernel/neuron/src/task/mod.rs` (task QoS metadata wiring if required)
- `source/kernel/neuron/src/sched/mod.rs` (scheduler QoS hint integration only if required)
- `source/libs/nexus-abi/`
- `source/services/timed/`
- `userspace/` (client lib only if required)
- `source/services/execd/` and/or `source/init/nexus-init/`
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/kernel/`
- `docs/services/`

### @task_0014_context
- `tasks/TASK-0014-observability-v2-metrics-tracing.md`
- `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md`
- `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `docs/observability/logging.md`
- `docs/observability/metrics.md`
- `docs/observability/tracing.md`

### @task_0014_touched
- `source/services/metricsd/`
- `userspace/nexus-metrics/`
- `source/libs/nexus-log/` (sink-logd wiring contract for deterministic slot configuration)
- `source/apps/selftest-client/`
- `source/services/policyd/`
- `source/services/rngd/`
- `source/services/keystored/`
- `source/services/statefsd/`
- `source/init/nexus-init/`
- `source/services/execd/`
- `source/services/bundlemgrd/`
- `source/services/dsoftbusd/`
- `source/services/timed/`
- `recipes/observability/metrics.toml`
- `recipes/policy/base.toml`
- `tools/nexus-idl/schemas/` (optional docs schema)
- `scripts/qemu-test.sh`
- `docs/observability/`

### @touched
- Only the directories listed in the task's **Touched paths** allowlist.

### @quality_gates
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

## Standard instruction line (recommended)
Kontext strikt: @core_context @task_context @quality_gates @touched. Kein @codebase Scan.

## Standard instruction line (TASK-0012)
Kontext strikt: @core_context @task_0012_context @quality_gates @task_0012_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0012B)
Kontext strikt: @core_context @task_0012b_context @quality_gates @task_0012b_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0013)
Kontext strikt: @core_context @task_0013_context @quality_gates @task_0013_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0014)
Kontext strikt: @core_context @task_0014_context @quality_gates @task_0014_touched. Kein @codebase Scan.
