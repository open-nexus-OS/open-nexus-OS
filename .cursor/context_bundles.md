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

### @task_0046_context
- `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`
- `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
- `docs/adr/0017-service-architecture.md`
- `tasks/TASK-0045-devx-nx-cli-v1.md`
- `docs/devx/nx-cli.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0046_touched
- `schemas/`
- `tools/nexus-idl/schemas/`
- `userspace/config/nexus-config/`
- `userspace/config/` (crate root for `nexus-config` package)
- `source/services/configd/`
- `tools/nx/`
- `tests/`
- `docs/config/`
- `docs/devx/nx-cli.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (only for OS-gated proof sync)
- `docs/rfcs/README.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- `CHANGELOG.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0047_context
- Status: `TASK-0047` / `RFC-0045` are `Done` at host-first closure after behavior-first remediation of configd `policy.root` carriage, external policyd frame API/audit, service-facing check cutover, required manifest validation, and honest `nx policy mode` preflight semantics; OS/QEMU policy markers remain gated and unclaimed.
- `tasks/TASK-0047-policy-as-code-v1-unified-engine.md`
- `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
- `docs/adr/0014-policy-architecture.md`
- `docs/adr/0017-service-architecture.md`
- `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
- `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md`
- `tasks/TASK-0045-devx-nx-cli-v1.md`
- `docs/devx/nx-cli.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0047_touched
- `policies/`
- `schemas/policy/`
- `recipes/policy/` (migration/parity only; must not remain a second live authority)
- `userspace/policy/`
- `source/services/policyd/`
- `tools/nx/`
- `tools/policy-gen/`
- `tests/`
- `docs/security/policy-as-code.md`
- `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
- `docs/rfcs/README.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0054_context
- Status: `TASK-0054` is `Done` and `RFC-0046` is `Done`; execution SSOT remains `TASK-0054`, contract seed is `RFC-0046`.
- Scope: host-first BGRA8888 CPU renderer, deterministic damage tracking, deterministic headless snapshots/goldens.
- Proof: `cargo test -p ui_renderer -- --nocapture`, `cargo test -p ui_host_snap -- --nocapture`, `cargo test -p ui_host_snap reject -- --nocapture`, `scripts/fmt-clippy-deny.sh`, `make clean`, `make build`, `make test`, and `make run` are green; no OS/QEMU renderer marker claim.
- Gate: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`); local TASK-0054 hardening is production-grade for bounds, ownership, type safety, and proof honesty, but no Gate A kernel/core production-grade claim is allowed.
- No OS/QEMU present markers, kernel changes, compositor, GPU, input routing, scheduler, MM, IPC, or VMO closure in this task.
- Escalate simplistic scheduler/MM/IPC/VMO/timer findings to `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0288`, `TASK-0290`, or a new RFC/task.
- `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`
- `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/architecture/nexusgfx-compute-and-executor-model.md`
- `docs/architecture/nexusgfx-text-pipeline.md`
- `docs/standards/RUST_STANDARDS.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/testing/index.md`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `.cursor/rules/06-testing-discipline.mdc`
- `.cursor/rules/20-arch-sync.mdc`

### @task_0054_touched
- `userspace/ui/renderer/`
- `userspace/ui/fonts/` (embedded deterministic fixture/fallback font data)
- `tests/ui_host_snap/`
- `docs/dev/ui/foundations/quality/testing.md`
- `Cargo.toml` (workspace membership for new crates/tests; protected path, explicitly justify before editing)
- `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md`
- `docs/rfcs/README.md`
- `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- `CHANGELOG.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/context_bundles.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0055_context
- Status: `RFC-0047` is `Done` (contract). `TASK-0055` is `Done` after execution/proof closure sync.
- Scope under review: OS-gated headless `windowd` surface/layer IPC seed, VMO-shaped surface buffer validation, bounded composition, minimal present acknowledgement, launcher first-frame client, and deterministic markers/postflight.
- Current repo reality: `source/services/windowd/` contains the bounded headless state machine; `userspace/apps/launcher/` exists; UI-present markers are wired through `selftest-client`, proof-manifest, `scripts/qemu-test.sh`, and `tools/postflight-ui.sh`.
- Proof: `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`, `cargo test -p ui_windowd_host reject -- --nocapture`, `cargo test -p ui_windowd_host capnp -- --nocapture`, `cargo test -p launcher -- --nocapture`, `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`, `scripts/fmt-clippy-deny.sh`, `make build` → `make test`, `make build` → `make run`.
- Gate: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`); `TASK-0055` contributes headless first-present/surface ownership, not visible display, input routing, or kernel/core production-grade closure.
- Security: fail closed for forged/missing VMO handles, wrong rights, invalid dimensions/stride/format, stale surface/commit IDs, unauthorized layer mutation, and fake markers.
- Follow-ups: visible scanout/present (`TASK-0055B/C`), dev presets (`TASK-0055D`), input/present evolution (`TASK-0056/B/C`), renderer abstraction (`TASK-0169/0170/0170B`), display backend (`TASK-0250/0251`).
- `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`
- `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
- `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md`
- `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/architecture/nexusgfx-compute-and-executor-model.md`
- `docs/architecture/nexusgfx-text-pipeline.md`
- `.cursor/rules/00-task-entry.mdc`
- `.cursor/rules/01-handoff-entry.mdc`
- `.cursor/rules/10-security-services.mdc`
- `.cursor/rules/20-arch-sync.mdc`

### @task_0055_touched
- `source/services/windowd/`
- `source/services/windowd/idl/`
- `userspace/apps/launcher/`
- `tests/ui_windowd_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui.sh`
- `scripts/qemu-test.sh`
- `docs/dev/ui/overview.md`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/dev/ui/foundations/layout/profiles.md`
- `Cargo.toml` / `Cargo.lock` only for workspace membership or generated metadata.
- Config/policy docs or manifests only if `ui.profile`, display dimensions, or service permissions are introduced.

### @task_0055b_context
- Status: `TASK-0055B` is `Done` and `RFC-0048` is `Done`; `TASK-0055`/`RFC-0047` remain closed headless carry-in baseline (`Done`).
- Scope closed: one deterministic QEMU `ramfb` visible bootstrap path selected by `NEXUS_DISPLAY_BOOTSTRAP=1`; fixed visible mode is `1280x800` ARGB8888 with `5120` byte stride.
- Profile boundary: `visible-bootstrap` is a proof-manifest harness/marker profile, not a future SystemUI/launcher start profile such as desktop/TV/mobile/car.
- Proof: `cargo test -p windowd -p ui_windowd_host -- --nocapture`, OS-target `selftest-client`/`init-lite` checks, and `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` are green for the slice.
- Gate: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` Gate E (`Windowing, UI & Graphics`, `production-floor`) visible-bootstrap contribution only; no visible SystemUI/input/perf/kernel production-grade claim.
- Security/authority: `windowd` remains mode/present/pattern/marker authority; `selftest-client` configures `ramfb` through policy-gated `device.mmio.fwcfg`; invalid mode/stride/format/capability handoff and pre-scanout markers fail closed.
- `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
- `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md`
- `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`
- `tasks/TASK-0010-device-mmio-access-model.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `.cursor/rules/10-security-services.mdc`

### @task_0055b_touched
- QEMU runner/harness configuration for graphics-capable UI boot
- QEMU `ramfb` / `fw_cfg` bootstrap mode
- `nexus-init` capability distribution and `policies/base.toml` for `device.mmio.fwcfg`
- `source/services/windowd/` (only as needed to target the visible buffer)
- `source/apps/selftest-client/`
- `source/apps/selftest-client/proof-manifest/`
- `docs/dev/ui/foundations/quality/testing.md`
- `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md`
- `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/context_bundles.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

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
- `docs/config/index.md`
- `scripts/qemu-test.sh`
- `docs/observability/logging.md`
- `docs/observability/metrics.md`
- `docs/observability/tracing.md`

### @task_0013b_context
- `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md`
- `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md`
- `docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md`
- `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`
- `tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0013b_touched
- `userspace/nexus-ipc/`
- `source/services/timed/`
- `source/services/metricsd/`
- `source/services/rngd/`
- `source/services/execd/`
- `source/services/keystored/`
- `source/services/statefsd/`
- `source/services/policyd/`
- `source/services/updated/`
- `source/apps/selftest-client/`
- `source/kernel/neuron/src/sched/`
- `scripts/qemu-test.sh`
- `docs/rfcs/`
- `tasks/`

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

### @task_0015_context
- `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
- `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
- `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
- `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
- `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @task_0015_touched
- `source/services/dsoftbusd/**`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `tools/os2vm.sh` (harness-only sync when required for deterministic proof parity)

### @task_0016_context
- `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `docs/testing/network-distributed-debugging.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @task_0016_touched
- `source/services/dsoftbusd/**`
- `userspace/dsoftbus/**` (only if strictly required by task scope)
- `source/services/packagefsd/**` (narrow RPC entry seam only if required)
- `userspace/remote-fs/remote-packagefs/**`
- `source/apps/selftest-client/**`
- `scripts/qemu-test.sh`
- `docs/distributed/**`

### @task_0017_context
- `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
- `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/testing/index.md`
- `docs/testing/network-distributed-debugging.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @task_0017_touched
- `source/services/dsoftbusd/**`
- `userspace/statefs/**`
- `userspace/remote-fs/remote-statefs/**`
- `source/apps/selftest-client/**`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`
- `docs/distributed/remote-fs.md`

### @task_0018_context
- `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`
- `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
- `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0018_touched
- `userspace/crash/**`
- `userspace/statefs/**` (only crashdump path client seams)
- `source/services/execd/**`
- `source/apps/selftest-client/**`
- `userspace/apps/**` (crash payload apps only)
- `tools/**` (host symbolization/minidump tooling only)
- `scripts/qemu-test.sh`
- `docs/observability/**`
- `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`

### @task_0019_context
- `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
- `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0019_touched
- `source/libs/nexus-abi/**`
- `source/services/policyd/**`
- `recipes/policy/**`
- `source/apps/selftest-client/**`
- `scripts/qemu-test.sh`
- `docs/security/abi-filters.md`
- `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
- `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`

### @task_0020_context
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
- `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
- `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @dsoftbus_production_closure_context
- `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
- `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md`
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- `docs/testing/index.md`
- `.cursor/current_state.md`
- `.cursor/stop_conditions.md`

### @task_0020_touched
- `userspace/dsoftbus/**`
- `source/services/dsoftbusd/**` (OS-gated integration only)
- `source/apps/selftest-client/**` (OS-gated markers only)
- `tests/**` (mux host tests)
- `docs/distributed/**`
- `scripts/qemu-test.sh` (only when OS backend gate is met)
- `tools/os2vm.sh` (2-VM mux + perf + soak marker/budget/hardening gates + release-evidence bundle)

### @task_0021_context
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @task_0021_touched
- `userspace/dsoftbus/**` (transport selection + host QUIC integration)
- `source/services/dsoftbusd/**` (selection/fallback wiring only)
- `source/apps/selftest-client/**` (fallback markers only)
- `tests/**` (requirement-named host QUIC/downgrade tests)
- `docs/distributed/**`
- `scripts/qemu-test.sh` (marker-contract sync only if required)

### @task_0022_context
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `.cursor/rules/07-behavior-first-proofs.mdc`

### @task_0022_touched
- `userspace/dsoftbus/**` (core/backend split + transport abstraction seams)
- `source/services/dsoftbusd/**` (integration seams only when required by contract)
- `tests/**` (core reject/bounds/state proofs)
- `docs/distributed/**`
- `docs/rfcs/**` (RFC-0036 + index sync)
- `scripts/qemu-test.sh` (only if marker contract changes)
- `tools/os2vm.sh` (only if distributed behavior claims require it)

### @task_0023_context
- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
- `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
- `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
- `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
- `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `scripts/qemu-test.sh`

### @task_0023_touched
- `userspace/dsoftbus/**` (host feasibility/reject suites and closure-proof sync)
- `userspace/net/nexus-net/**` (only if tightly required by closure-proof contract)
- `source/services/dsoftbusd/**` (real OS session path + marker contract surfaces)
- `source/apps/selftest-client/**` (real QUIC session probe path)
- `tests/**` (requirement-named reject/feasibility suites)
- `docs/distributed/**`
- `docs/rfcs/**` (RFC-0037 seed + index sync)
- `scripts/qemu-test.sh`

### @task_0023b_context
- `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
- `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
- `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
- `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/testing/index.md`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `scripts/qemu-test.sh`

### @task_0023b_touched
- `source/apps/selftest-client/src/main.rs`
- `source/apps/selftest-client/src/**`
- `docs/testing/index.md` (only if proof command guidance changes)
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

### @task_0029_context
- `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md`
- `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md`
- `docs/adr/0020-manifest-format-capnproto.md`
- `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
- `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- `docs/rfcs/RFC-0016-device-identity-keys-v1.md`
- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
- `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/security/signing-and-policy.md`
- `docs/packaging/nxb.md`
- `docs/packaging/system-set.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `tools/nexus-idl/schemas/keystored.capnp`
- `source/libs/nexus-evidence/` (READ-ONLY — reuse `scan` + reproducible-tar primitives)
- `source/apps/selftest-client/proof-manifest/` (markers + profile registration)
- `.cursor/rules/12-debug-discipline.mdc`
- `scripts/qemu-test.sh`

### @task_0029_touched
- `tools/nxb-pack/` (embed SBOM + repro using `nexus-evidence` reproducible-tar primitives)
- `tools/sbom/` (new: CycloneDX 1.5 JSON generator)
- `tools/repro/` (new: repro metadata capture + `repro-verify`)
- `source/services/bundlemgrd/` (install-time enforcement; routes via `policyd`)
- `source/services/keystored/` (allowlist check + key registry API impl)
- `source/services/policyd/` (allow/deny decision + audit context)
- `tools/nexus-idl/schemas/keystored.capnp` (**ABI** — CAUTION zone)
- `recipes/signing/` (new allowlist TOML)
- `tests/` (host tests, including `test_reject_*` set)
- `docs/supplychain/` (new docs: `sbom.md`, `repro.md`, `sign-policy.md`)
- `docs/testing/index.md` (host commands + gated OS markers)
- `scripts/qemu-test.sh` (gated marker update only)
- `source/apps/selftest-client/proof-manifest/markers/` (new markers)
- `source/apps/selftest-client/proof-manifest/profiles/` (profile registration)
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `docs/rfcs/README.md` (RFC-0039 index entry on closure)

### @task_0031_context
- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/standards/RUST_STANDARDS.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `.cursor/rules/07-behavior-first-proofs.mdc`
- `.cursor/rules/12-debug-discipline.mdc`

### @task_0031_touched
- `userspace/memory/**` (new `nexus-vmo` crate)
- `source/apps/selftest-client/**` (cross-process VMO proof path + markers)
- `docs/storage/vmo.md` (new contract doc)
- `docs/testing/index.md` (proof contract sync only)
- `scripts/qemu-test.sh` (marker/profile sync only)
- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0032_context
- `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md`
- `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md`
- `tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md`
- `tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/architecture/12-storage-vfs-packagefs.md`
- `docs/packaging/nxb.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0032_touched
- `source/services/packagefsd/**`
- `userspace/storage/**` (pkgimg format/parsing helpers)
- `tools/pkgimg-build/**` (new host builder tool)
- `tools/nxb-pack/**` (only if integration wiring is required)
- `source/apps/selftest-client/**` (gated pkgimg markers only)
- `docs/architecture/12-storage-vfs-packagefs.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (marker/profile sync only)
- `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md`
- `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0039_context
- `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
- `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md`
- `tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

### @task_0039_touched
- `source/services/vfsd/**`
- `source/services/execd/**`
- `source/services/keystored/**` (only if key-custody integration is required)
- `userspace/nexus-vfs/**`
- `source/apps/selftest-client/**` (OS-gated markers only)
- `tests/**` (host reject/confinement proofs)
- `docs/security/sandboxing.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (marker/profile sync only)
- `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
- `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/context_bundles.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0045_context
- `tasks/TASK-0045-devx-nx-cli-v1.md`
- `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/README.md`
- `docs/testing/index.md`
- `justfile`

### @task_0045_touched
- `tools/nx/**` (new crate + subcommand surface)
- `tools/nx/templates/**` (new)
- `tools/nexus-idl/**` (optional minimal v1 helper alignment only)
- `docs/devx/nx-cli.md` (new)
- `docs/testing/index.md`
- `tasks/TASK-0045-devx-nx-cli-v1.md`
- `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md`
- `docs/rfcs/README.md`
- `.cursor/current_state.md`
- `.cursor/handoff/current.md`
- `.cursor/context_bundles.md`
- `.cursor/next_task_prep.md`
- `.cursor/pre_flight.md`
- `.cursor/stop_conditions.md`

### @task_0016b_context
- `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
- `tasks/TASK-0010-device-mmio-access-model.md`
- `tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md`
- `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
- `tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md`
- `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
- `docs/rfcs/RFC-0006-userspace-networking-v1.md`
- `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `docs/testing/index.md`
- `docs/testing/network-distributed-debugging.md`
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`

### @task_0016b_touched
- `source/services/netstackd/**`
- `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
- `docs/rfcs/README.md`
- `docs/testing/index.md` (only if proof/developer guidance would otherwise drift)
- `scripts/qemu-test.sh` (only if gate definitions need sync without semantic drift)
- `tools/os2vm.sh` (only if regression-harness sync is required without semantic drift)

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

## Standard instruction line (TASK-0013B)
Kontext strikt: @core_context @task_0013b_context @quality_gates @task_0013b_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0015)
Kontext strikt: @core_context @task_0015_context @quality_gates @task_0015_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0016)
Kontext strikt: @core_context @task_0016_context @quality_gates @task_0016_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0017)
Kontext strikt: @core_context @task_0017_context @quality_gates @task_0017_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0018)
Kontext strikt: @core_context @task_0018_context @quality_gates @task_0018_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0019)
Kontext strikt: @core_context @task_0019_context @quality_gates @task_0019_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0020)
Kontext strikt: @core_context @task_0020_context @quality_gates @task_0020_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0021)
Kontext strikt: @core_context @task_0021_context @quality_gates @task_0021_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0022)
Kontext strikt: @core_context @task_0022_context @quality_gates @task_0022_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0023)
Kontext strikt: @core_context @task_0023_context @quality_gates @task_0023_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0023B)
Kontext strikt: @core_context @task_0023b_context @quality_gates @task_0023b_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0029)
Kontext strikt: @core_context @task_0029_context @quality_gates @task_0029_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0031)
Kontext strikt: @core_context @task_0031_context @quality_gates @task_0031_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0032)
Kontext strikt: @core_context @task_0032_context @quality_gates @task_0032_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0039)
Kontext strikt: @core_context @task_0039_context @quality_gates @task_0039_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0045)
Kontext strikt: @core_context @task_0045_context @quality_gates @task_0045_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0054)
Kontext strikt: @core_context @task_0054_context @quality_gates @task_0054_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0055B)
Kontext strikt: @core_context @task_0055b_context @quality_gates @task_0055b_touched. Kein @codebase Scan.

## Standard instruction line (DSoftBus production closure)
Kontext strikt: @core_context @dsoftbus_production_closure_context @quality_gates @task_0020_touched. Kein @codebase Scan.

## Standard instruction line (TASK-0016B)
Kontext strikt: @core_context @task_0016b_context @quality_gates @task_0016b_touched. Kein @codebase Scan.

### @network_distributed_debug_context
- `docs/testing/network-distributed-debugging.md`
- `tools/os2vm.sh`
- `scripts/qemu-test.sh`
- `source/services/dsoftbusd/src/os/session/cross_vm.rs`
- `source/services/dsoftbusd/src/os/netstack/stream_io.rs`

## Standard instruction line (TASK-0016 runtime triage)
Kontext strikt: @core_context @task_0016_context @network_distributed_debug_context @quality_gates @task_0016_touched. Kein @codebase Scan.
