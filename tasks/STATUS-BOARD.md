# Status Board: Task and Track Overview

This board stays intentionally simple:

- one **ongoing Done list** (not capped to a fixed range),
- one **complete TRACK index**.

Source of truth for task status remains each `tasks/TASK-*.md` header.
Sequential execution order remains `tasks/IMPLEMENTATION-ORDER.md`.

---

## Done (Ongoing, Cumulative)

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| ✅ TASK-0001 | Runtime roles & boundaries | Done | Single-authority model locked |
| ✅ TASK-0002 | Userspace VFS proof | Done | Marker-gated proof in QEMU |
| ✅ TASK-0003 | Networking: virtio-net + smoltcp + dsoftbusd | Done | OS transport complete |
| ✅ TASK-0003B | DSoftBus Noise XK OS | Done | Handshake + identity binding |
| ✅ TASK-0003C | DSoftBus UDP discovery OS | Done | Loopback discovery |
| ✅ TASK-0004 | Networking: dual-node + identity binding | Done | Identity enforcement |
| ✅ TASK-0005 | Cross-VM DSoftBus + remote proxy | Done | 2-VM harness established |
| ✅ TASK-0006 | Observability v1: logd + crash reports | Done | Journal + nexus-log sink |
| ✅ TASK-0007 | Updates & Packaging v1.0 | Done | A/B skeleton + markers |
| ✅ TASK-0008 | Security hardening v1: policy + audit | Done | Policy engine + audit trail |
| ✅ TASK-0008B | Device identity keys v1 | Done | Keygen flow complete |
| ✅ TASK-0009 | Persistence v1: virtio-blk + statefs | Done | State persistence baseline complete |
| ✅ TASK-0010 | Device MMIO access model | Done | Capability-gated device access complete |
| ✅ TASK-0011 | Kernel simplification phase A | Done | Simplification baseline complete |
| ✅ TASK-0011B | Kernel Rust idioms pre-SMP | Done | Idiom cleanup complete |
| ✅ TASK-0012 | Kernel SMP v1 | Done | Baseline complete |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge | Done | Hardening complete |
| ✅ TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | Done | QoS/timing contract complete |
| ✅ TASK-0013B | IPC liveness hardening v1 | Done | Bounded retry/correlation proof complete |
| ✅ TASK-0014 | Observability v2: metrics + tracing | Done | Local observability v2 complete |
| ✅ TASK-0015 | DSoftBusd refactor v1: modular OS daemon structure | Done | Modular daemon baseline complete |
| ✅ TASK-0016 | DSoftBus Remote-FS v1: Remote PackageFS proxy | Done | RFC-0028 gates complete |
| ✅ TASK-0016B | Netstackd refactor v1: modular structure + loop hardening | Done | Seam and governance sync complete |
| ✅ TASK-0017 | DSoftBus Remote-StateFS v1 | Done | Deterministic ACL/audit + 1-VM/2-VM proof complete |
| ✅ TASK-0018 | Crashdumps v1: deterministic minidump + host symbolization | Done | Final hardening + drift lock complete |

Current queue head (in review): `TASK-0019`.

---

## Planned UI/DSL Insertions

These draft tasks intentionally create an earlier visible UI/DSL path so app and SystemUI work can be tested in a real
QEMU window before the later display/system migration tasks fully land.

| Task | Purpose |
|------|---------|
| TASK-0054B | kernel/UI perf floor (zero-copy + trusted scheduling + SMP hardening carry-ins) |
| TASK-0054C | kernel IPC fastpath v1 for short control messages |
| TASK-0054D | kernel MM perf floor for VMO/surface reuse |
| TASK-0055B | visible QEMU scanout bootstrap |
| TASK-0055C | `windowd` visible present + SystemUI first frame |
| TASK-0055D | deterministic QEMU dev display/profile presets (`phone/tablet/laptop/laptop-pro/convertible` + orientation + shell mode + Hz) |
| TASK-0056B | visible input v0 (cursor/focus/click) |
| TASK-0056C | present/input perf polish (latency + coalescing + skip paths) |
| TASK-0060B | glass materials + backdrop cache + deterministic degrade |
| TASK-0062B | animation frame-budget discipline + canonical perf scenes |
| TASK-0067B | clipboard history DSL overlay/app |
| TASK-0076B | visible DSL OS mount + first DSL frame |
| TASK-0080B | bootstrap SystemUI DSL shell (host-first) |
| TASK-0080C | bootstrap SystemUI DSL shell (OS/QEMU) |
| TASK-0100B | Audio Mixer DSL app/SystemUI surface |
| TASK-0122B | shared DSL app platform |
| TASK-0122C | shared DSL app integration kit |

---

## RFC Done (Ongoing, Cumulative)

| RFC | Description | File |
|-----|-------------|------|
| ✅ RFC-0001 | Kernel Simplification | `docs/rfcs/RFC-0001-kernel-simplification.md` |
| ✅ RFC-0002 | Process-Per-Service Architecture | `docs/rfcs/RFC-0002-process-per-service-architecture.md` |
| ✅ RFC-0003 | Unified Logging Infrastructure | `docs/rfcs/RFC-0003-unified-logging.md` |
| ✅ RFC-0004 | Loader Safety & Shared-Page Guards | `docs/rfcs/RFC-0004-safe-loader-guards.md` |
| ✅ RFC-0005 | Kernel IPC & Capability Model | `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` |
| ✅ RFC-0006 | Userspace Networking v1 | `docs/rfcs/RFC-0006-userspace-networking-v1.md` |
| ✅ RFC-0007 | DSoftBus OS Transport v1 | `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md` |
| ✅ RFC-0008 | DSoftBus Noise XK v1 | `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md` |
| ✅ RFC-0009 | no_std Dependency Hygiene v1 | `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md` |
| ✅ RFC-0010 | DSoftBus Cross-VM Harness v1 | `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md` |
| ✅ RFC-0011 | logd journal + crash reports v1 | `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` |
| ✅ RFC-0012 | Updates & Packaging v1.0 (A/B skeleton) | `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` |
| ✅ RFC-0013 | Boot gates v1 | `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` |
| ✅ RFC-0014 | Testing contracts v1 | `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` |
| ✅ RFC-0015 | Policy Authority & Audit Baseline v1 | `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` |
| ✅ RFC-0016 | Device Identity Keys v1 | `docs/rfcs/RFC-0016-device-identity-keys-v1.md` |
| ✅ RFC-0017 | Device MMIO Access Model v1 | `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` |
| ✅ RFC-0018 | StateFS Journal Format v1 | `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` |
| ✅ RFC-0019 | IPC Request/Reply Correlation v1 | `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` |
| ✅ RFC-0020 | Kernel ownership + Rust idioms pre-SMP v1 | `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` |
| ✅ RFC-0021 | Kernel SMP v1 contract | `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` |
| ✅ RFC-0022 | Kernel SMP v1b hardening contract | `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md` |
| ✅ RFC-0023 | QoS ABI + timed coalescing contract v1 | `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md` |
| ✅ RFC-0024 | Observability v2 local contract | `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md` |
| ✅ RFC-0025 | IPC liveness hardening v1 | `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md` |
| ✅ RFC-0026 | IPC performance optimization v1 | `docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md` |
| ✅ RFC-0027 | DSoftBusd modular daemon structure v1 | `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` |
| ✅ RFC-0028 | DSoftBus remote packagefs RO v1 | `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md` |
| ✅ RFC-0029 | Netstackd modular daemon structure v1 | `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` |
| ✅ RFC-0030 | DSoftBus remote statefs RW v1 | `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md` |
| ✅ RFC-0031 | Crashdumps v1 + host symbolization | `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md` |
| ✅ RFC-0032 | ABI syscall guardrails v2 (userland, kernel-untouched) | `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` |

Current RFC queue head (in progress): none (RFC-0032 complete).

---

## TRACK Index (Complete List)

| Track | File |
|-------|------|
| TRACK-ADS-SAFETY-FAMILYMODE | `tasks/TRACK-ADS-SAFETY-FAMILYMODE.md` |
| TRACK-APP-STORE | `tasks/TRACK-APP-STORE.md` |
| TRACK-ARCADE-APP | `tasks/TRACK-ARCADE-APP.md` |
| TRACK-AUTHORITY-NAMING | `tasks/TRACK-AUTHORITY-NAMING.md` |
| TRACK-CORE-UTILITIES | `tasks/TRACK-CORE-UTILITIES.md` |
| TRACK-CREATIVE-APPS | `tasks/TRACK-CREATIVE-APPS.md` |
| TRACK-DAW-APP | `tasks/TRACK-DAW-APP.md` |
| TRACK-DEVSTUDIO-IDE | `tasks/TRACK-DEVSTUDIO-IDE.md` |
| TRACK-DRIVERS-ACCELERATORS | `tasks/TRACK-DRIVERS-ACCELERATORS.md` |
| TRACK-DSL-V1-DEVX | `tasks/TRACK-DSL-V1-DEVX.md` |
| TRACK-FEEDS-APP | `tasks/TRACK-FEEDS-APP.md` |
| TRACK-KEYSTONE-GATES | `tasks/TRACK-KEYSTONE-GATES.md` |
| TRACK-LIVE-STUDIO-APP | `tasks/TRACK-LIVE-STUDIO-APP.md` |
| TRACK-LOCATION-STACK | `tasks/TRACK-LOCATION-STACK.md` |
| TRACK-MAIL-APP | `tasks/TRACK-MAIL-APP.md` |
| TRACK-MAPS-APP | `tasks/TRACK-MAPS-APP.md` |
| TRACK-MEDIA-APPS | `tasks/TRACK-MEDIA-APPS.md` |
| TRACK-NETWORKING-DRIVERS | `tasks/TRACK-NETWORKING-DRIVERS.md` |
| TRACK-NEXUSACCOUNT | `tasks/TRACK-NEXUSACCOUNT.md` |
| TRACK-NEXUSFRAME | `tasks/TRACK-NEXUSFRAME.md` |
| TRACK-NEXUSGAME-SDK | `tasks/TRACK-NEXUSGAME-SDK.md` |
| TRACK-NEXUSGFX-SDK | `tasks/TRACK-NEXUSGFX-SDK.md` |
| TRACK-NEXUSMEDIA-SDK | `tasks/TRACK-NEXUSMEDIA-SDK.md` |
| TRACK-NEXUSNET-SDK | `tasks/TRACK-NEXUSNET-SDK.md` |
| TRACK-NEXUSSOCIAL | `tasks/TRACK-NEXUSSOCIAL.md` |
| TRACK-NEXUSVIDEO | `tasks/TRACK-NEXUSVIDEO.md` |
| TRACK-NOTES-APP | `tasks/TRACK-NOTES-APP.md` |
| TRACK-OFFICE-SUITE | `tasks/TRACK-OFFICE-SUITE.md` |
| TRACK-PASSWORD-MANAGER | `tasks/TRACK-PASSWORD-MANAGER.md` |
| TRACK-PIM-SUITE | `tasks/TRACK-PIM-SUITE.md` |
| TRACK-PINBALL-APP | `tasks/TRACK-PINBALL-APP.md` |
| TRACK-PODCASTS-APP | `tasks/TRACK-PODCASTS-APP.md` |
| TRACK-PUZZLE-APP | `tasks/TRACK-PUZZLE-APP.md` |
| TRACK-RECIPES-APP | `tasks/TRACK-RECIPES-APP.md` |
| TRACK-REFERENCE-GAMES | `tasks/TRACK-REFERENCE-GAMES.md` |
| TRACK-REMOVABLE-STORAGE | `tasks/TRACK-REMOVABLE-STORAGE.md` |
| TRACK-SCORE-APP | `tasks/TRACK-SCORE-APP.md` |
| TRACK-SYSTEM-DELEGATION | `tasks/TRACK-SYSTEM-DELEGATION.md` |
| TRACK-TELEPROMPTER-APP | `tasks/TRACK-TELEPROMPTER-APP.md` |
| TRACK-TERMINAL-APP | `tasks/TRACK-TERMINAL-APP.md` |
| TRACK-VIDEO-EDITOR-APP | `tasks/TRACK-VIDEO-EDITOR-APP.md` |
| TRACK-WEATHER-APP | `tasks/TRACK-WEATHER-APP.md` |
| TRACK-ZEROCOPY-APP-PLATFORM | `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md` |

---

## Related

- Sequential execution order: `tasks/IMPLEMENTATION-ORDER.md`
- Task workflow rules: `tasks/README.md`
- RFC process: `docs/rfcs/README.md`
