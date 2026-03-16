# Status Board: Task Progress View

This file provides a **Kanban-style status view** over tasks.

**Source of truth**: Each `tasks/TASK-*.md` file (status field in YAML header).

For sequential execution order, see: `tasks/IMPLEMENTATION-ORDER.md`.

---

## How to Read This Board

- **Status** comes from the task's YAML header (`status:` field)
- **Tasks execute in numerical order** (TASK-0001, TASK-0002, ...)
- **TRACKs** are vision documents that spawn tasks — they don't have status themselves

---

## Done (TASK-0001 through TASK-0014)

| Task | Title | RFC | Completed | Notes |
|------|-------|-----|-----------|-------|
| ✅ TASK-0001 | Runtime roles & boundaries | — | 2025-12 | Single-authority model locked |
| ✅ TASK-0002 | Userspace VFS proof | — | 2025-12 | Marker-gated proof in QEMU |
| ✅ TASK-0003 | Networking: virtio-net + smoltcp + dsoftbusd | RFC-0006, RFC-0007 | 2026-01-07 | OS transport complete |
| ✅ TASK-0003B | DSoftBus Noise XK OS | RFC-0008 | 2026-01-07 | Handshake + identity binding |
| ✅ TASK-0003C | DSoftBus UDP discovery OS | RFC-0007 | 2026-01-07 | Loopback discovery |
| ✅ TASK-0004 | Networking: dual-node + identity binding | RFC-0007, RFC-0008 | 2026-01-10 | Identity enforcement |
| ✅ TASK-0005 | Cross-VM DSoftBus + remote proxy | RFC-0010 | 2026-01-13 | 2-VM harness (opt-in) |
| ✅ TASK-0006 | Observability v1: logd + crash reports | RFC-0011 | 2026-01-14 | Journal + nexus-log sink |
| ✅ TASK-0007 | Updates & Packaging v1.0 | RFC-0012 | 2026-01-20 | A/B skeleton + markers |
| ✅ TASK-0008 | Security hardening v1: policy + audit | RFC-0015 | 2026-01-25 | Policy engine + audit trail |
| ✅ TASK-0008B | Device identity keys v1 | — | — | Completed |
| ✅ TASK-0009 | Persistence v1: virtio-blk + statefs | — | — | Completed |
| ✅ TASK-0010 | Device MMIO access model | — | — | Completed |
| ✅ TASK-0011 | Kernel simplification phase A | RFC-0001 | — | Completed |
| ✅ TASK-0011B | Kernel Rust idioms pre-SMP | — | — | Completed |
| ✅ TASK-0012 | Kernel SMP v1 | — | — | Baseline complete |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge | — | — | Hardening complete |
| ✅ TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | — | — | Completed |
| ✅ TASK-0013B | IPC liveness hardening v1 (bounded retry/correlation) | RFC-0025, RFC-0026 | — | Closed with review package + sequential proof discipline documented |
| ✅ TASK-0014 | Observability v2: metrics + tracing | — | — | Completed |

---

## Next

| Task | Title | Status | Blocked by | Next action |
|------|-------|--------|------------|-------------|
| **TASK-0015** | DSoftBusd refactor v1: modular OS daemon structure | Done | — | Closed: modular daemon baseline + proof package complete |
| TASK-0016 | DSoftBus Remote-FS v1: Remote PackageFS proxy | Draft | TASK-0005 | Start as next execution slice |
| TASK-0017 | DSoftBus Remote-StateFS v1 | Draft | TASK-0005 | Queue after Remote-FS planning |
| TASK-0020 | DSoftBus Streams v2: multiplexing + flow control + keepalive | Draft | TASK-0005 | Host-first mux work after refactor groundwork |
| TASK-0021 | DSoftBus QUIC v1 | Draft | TASK-0003, TASK-0005, TASK-0020 | Keep OS fallback path gated |
| TASK-0022 | DSoftBus core refactor: no_std-compatible core + transport abstraction | Draft | — | Reuse the seams prepared by TASK-0015 |

---

## RFCs Status

| RFC | Title | Status | Task |
|-----|-------|--------|------|
| ✅ RFC-0001 | Kernel Simplification | Complete | TASK-0011 |
| ✅ RFC-0002 | Process-Per-Service | Complete | — |
| ✅ RFC-0003 | Unified Logging | Complete | TASK-0006 |
| ✅ RFC-0004 | Loader Safety & Guards | Complete | — |
| ✅ RFC-0005 | Kernel IPC & Capability Model | Complete | — |
| ✅ RFC-0006 | Userspace Networking v1 | Complete | TASK-0003 |
| ✅ RFC-0007 | DSoftBus OS Transport v1 | Complete | TASK-0003, TASK-0004 |
| ✅ RFC-0008 | DSoftBus Noise XK v1 | Complete | TASK-0003B |
| ✅ RFC-0009 | no_std Dependency Hygiene v1 | Complete | — |
| ✅ RFC-0010 | DSoftBus Cross-VM Harness v1 | Complete | TASK-0005 |
| ✅ RFC-0011 | logd journal + crash reports v1 | Complete | TASK-0006 |
| ✅ RFC-0012 | Updates & Packaging v1.0 | Complete | TASK-0007 |
| ✅ RFC-0013 | Boot gates v1 | Complete | — |
| ✅ RFC-0014 | Testing contracts v1 | Complete | — |
| ✅ RFC-0015 | Policy Authority & Audit v1 | Complete | TASK-0008 |
| 🟨 RFC-0025 | IPC liveness hardening v1 | In Review | TASK-0013B |
| 🟨 RFC-0026 | IPC performance optimization v1 | In Review | TASK-0013B |
| ✅ RFC-0027 | DSoftBusd modular daemon structure v1 | Completed | TASK-0015 |

---

## TRACKs (Vision Documents)

TRACKs define feature areas but don't execute directly. They spawn tasks when gates clear.

| Track | Purpose | Gates (blocked by) |
|-------|---------|-------------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers | TASK-0010, TASK-0012B |
| TRACK-NEXUSGFX-SDK | Graphics SDK | UI tasks |
| TRACK-NEXUSMEDIA-SDK | Media SDK | UI + codec tasks |
| TRACK-ZEROCOPY-APP-PLATFORM | App platform | TASK-0031, clipboard |
| TRACK-APP-STORE | Distribution | Packaging tasks |
| TRACK-DEVSTUDIO-IDE | Developer IDE | DSL tasks |
| TRACK-OFFICE-SUITE | Office apps | UI + OpLog |
| TRACK-PIM-SUITE | PIM apps (calendar, contacts) | UI + sync |
| TRACK-MEDIA-APPS | Media apps (photos, music, video) | Media SDK |

---

## Workflow

1. **Execute tasks in numerical order** (TASK-0001, TASK-0002, ...)
2. If a task is blocked, skip to the next unblocked task
3. TRACKs spawn new tasks when their gates clear
4. New tasks get the next available number
5. Mark task `Done` only when all stop conditions are met

---

## Related

- **Sequential order**: `tasks/IMPLEMENTATION-ORDER.md`
- **Task workflow rules**: `tasks/README.md`
- **RFC process**: `docs/rfcs/README.md`
