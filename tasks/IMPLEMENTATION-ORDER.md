# Implementation Order: Sequential by Task Number

This file provides a **sequential execution view** over `tasks/TASK-*.md`.

**Primary rule**: Tasks are executed in **numerical order** (TASK-0001, TASK-0002, ...).

This file is **not authoritative** for scope/DoD; each `TASK-*.md` remains execution truth.

For Kanban-style status view, see: `tasks/STATUS-BOARD.md`.

---

## How Tasks and TRACKs Work Together

### Tasks (`TASK-XXXX-*.md`)

- **Atomic work units** with clear stop conditions (Definition of Done)
- Executed in **numerical order**
- Each task has proofs (host tests, QEMU markers)
- Status: `Draft` → `In Progress` → `In Review` → `Done`

### Tracks (`TRACK-*.md`)

- **Vision documents** that describe a larger feature area or product direction
- TRACKs are **not executed directly** — they spawn tasks
- A TRACK contains:
  - High-level goals and constraints
  - Candidate tasks (`CAND-*`) to be extracted into real `TASK-XXXX` files
  - Gates (RED/YELLOW/GREEN) that block extraction
  - Phase map showing progression

**Workflow**:
1. TRACKs define **what** we want to build (vision + constraints)
2. When a TRACK's gates are satisfied, extract a `CAND-*` into a real `TASK-XXXX`
3. The new task gets the next available number and enters the sequential queue
4. Execute tasks in numerical order

**Example**:
- `TRACK-DRIVERS-ACCELERATORS.md` defines the GPU/NPU/VPU vision
- Once `TASK-0010` (MMIO) and `TASK-0031` (VMO) are done, `CAND-DRV-000` can become `TASK-0280`
- `TASK-0280` then executes in its numerical position

---

## Done (Tasks 0001–0008)

| Task | Title | Completed |
|------|-------|-----------|
| ✅ TASK-0001 | Runtime roles & boundaries | — |
| ✅ TASK-0002 | Userspace VFS proof | — |
| ✅ TASK-0003 | Networking: virtio-net + smoltcp + dsoftbusd OS | — |
| ✅ TASK-0003B | DSoftBus Noise XK OS | — |
| ✅ TASK-0003C | DSoftBus UDP discovery OS | — |
| ✅ TASK-0004 | Networking: DHCP/ICMP + dual-node identity | — |
| ✅ TASK-0005 | Cross-VM DSoftBus + remote proxy | — |
| ✅ TASK-0006 | Observability v1: logd journal + crash reports | — |
| ✅ TASK-0007 | Updates & Packaging v1.0: A/B skeleton | — |
| ✅ TASK-0008 | Security hardening v1: policy engine + audit trail | 2026-01-25 |

---

## Current: TASK-0008B → TASK-0009 and onward

Execute in numerical order. Current position: **TASK-0008B**.

| Task | Title | Prereqs | Status |
|------|-------|---------|--------|
| **TASK-0008B** | Device identity keys v1 (virtio-rng + rngd + keystored keygen) | TASK-0008, TASK-0010 | Next |
| TASK-0009 | Persistence v1 (virtio-blk + statefs) | TASK-0008B, TASK-0010 | Queued |
| TASK-0010 | Device MMIO access model | — | Queued |
| TASK-0011 | Kernel simplification phase A | — | Queued |
| TASK-0011B | Kernel Rust idioms pre-SMP | TASK-0011 | Queued |
| TASK-0012 | Kernel SMP v1 (per-CPU runqueues + IPIs) | TASK-0011, TASK-0011B | Queued |
| TASK-0012B | Kernel SMP v1b hardening bridge (scheduler + SMP internals) | TASK-0012 | Queued |
| TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | TASK-0012B | Queued |
| TASK-0014 | Observability v2: metrics + tracing | TASK-0006 | In Review |

---

## Queue (TASK-0015+)

Continue in numerical order after TASK-0014.

Notable upcoming tasks:
- **TASK-0016–0024**: DSoftBus advanced features (remote packagefs, statefs, QUIC, etc.)
- **TASK-0025–0028**: StateFS hardening + ABI filters v2
- **TASK-0029**: Supply chain v1 (SBOM + signing policy)
- **TASK-0031**: Zero-copy VMOs v1 (enables driver + graphics tracks)
- **TASK-0039**: Sandboxing v1
- **TASK-0054+**: UI stack

---

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | Blocked by |
|-------|---------|------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU device-class services | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers, offload, netdevd | TASK-0003, TASK-0010, TASK-0012B |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | UI tasks (0054+) |
| TRACK-NEXUSMEDIA-SDK | Audio/video/image SDK | UI tasks, codec tasks |
| TRACK-ZEROCOPY-APP-PLATFORM | RichContent + OpLog + connectors | TASK-0031, TASK-0087 |
| TRACK-APP-STORE | Distribution + publishing | Packaging tasks |
| TRACK-DEVSTUDIO-IDE | Developer IDE | DSL tasks (0075+) |

---

## Rules

1. **Sequential by number**: Execute TASK-XXXX in order (0001, 0002, 0003, ...)
2. **Skip if blocked**: If a task has unsatisfied prereqs, note it and move to the next
3. **TRACKs don't execute**: TRACKs spawn tasks; the spawned task gets the next number
4. **100% rule**: Only mark a task Done when all stop conditions are met
5. **No fake success**: Markers/proofs must reflect real behavior

---

## Related

- **Status board (Kanban view)**: `tasks/STATUS-BOARD.md`
- **Task workflow rules**: `tasks/README.md`
- **RFC process**: `docs/rfcs/README.md`
