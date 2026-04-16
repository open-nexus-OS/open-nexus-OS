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

## Done (Tasks 0001–0014)

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
| ✅ TASK-0008B | Device identity keys v1 (virtio-rng + rngd + keystored keygen) | — |
| ✅ TASK-0009 | Persistence v1 (virtio-blk + statefs) | — |
| ✅ TASK-0010 | Device MMIO access model | — |
| ✅ TASK-0011 | Kernel simplification phase A | — |
| ✅ TASK-0011B | Kernel Rust idioms pre-SMP | — |
| ✅ TASK-0012 | Kernel SMP v1 (per-CPU runqueues + IPIs) | — |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge (scheduler + SMP internals) | — |
| ✅ TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | — |
| ✅ TASK-0013B | IPC liveness hardening v1: bounded retry/correlation | — |
| ✅ TASK-0014 | Observability v2: metrics + tracing | — |

---

## Current: TASK-0023 complete, TASK-0023B refactor next, TASK-0024 after

Execute in numerical order. Current queue head is **TASK-0023B (Draft, selftest-client refactor slice before transport feature expansion)**.
Latest completed closure slices before this queue head: **TASK-0020 (Done)** and **TASK-0021 (Done)**.
Current TASK-0020 closure checkpoint: requirement-based host contract/integration suites are green, canonical OS harnesses are green, mux marker ladders are proven in single-VM and 2-VM paths, deterministic perf and hardening soak gates are green, and a machine-readable release evidence bundle is emitted per run.
Current TASK-0023 closure checkpoint: host gate proofs (`just test-dsoftbus-quic`, `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`, `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`, `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`) and OS QUIC marker proof (`REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`) are green; required markers are `dsoftbusd: transport selected quic`, `dsoftbusd: auth ok`, `dsoftbusd: os session ok`, and `SELFTEST: quic session ok`, with fallback markers forbidden in this profile.
Current pre-feature queue policy: execute `TASK-0023B` first to refactor `source/apps/selftest-client/src/main.rs` into maintainable modules before implementing additional transport features in `TASK-0024`.
Production closure contract checkpoint: RFC-0034 is done for legacy TASK-0001..0020 production closure scope.

| Task | Title | Prereqs | Status |
|------|-------|---------|--------|
| **TASK-0015** | DSoftBusd refactor v1: modular OS daemon structure without behavior change | — | Done |
| **TASK-0016** | DSoftBus Remote-FS v1: Remote PackageFS proxy (read-only) over authenticated streams | TASK-0005 | Done |
| **TASK-0016B** | Netstackd refactor v1: modular OS daemon structure + loop/idiom hardening | TASK-0003, TASK-0010 | Done |
| TASK-0017 | DSoftBus Remote-StateFS v1 | TASK-0005 | Done |
| TASK-0018 | Crashdumps v1: minidump + host symbolization | TASK-0006, TASK-0009 | Done |
| TASK-0019 | Security v2 (OS): userland ABI syscall guardrails | TASK-0006, TASK-0008, TASK-0009 | Done |
| TASK-0020 | DSoftBus Streams v2: multiplexing + flow control + keepalive | TASK-0005 | Done |
| TASK-0021 | DSoftBus QUIC v1: host QUIC transport + OS UDP scaffold + TCP fallback | TASK-0003, TASK-0005, TASK-0020 | Done |
| TASK-0022 | DSoftBus core refactor: no_std-compatible core + transport abstraction | — | Done |
| TASK-0023 | DSoftBus QUIC v2 OS enablement (session path closure) | TASK-0003, TASK-0020, TASK-0022 | Done |
| TASK-0023B | Selftest-client production-grade deterministic test architecture refactor v1 | TASK-0023 | Draft |
| TASK-0024 | DSoftBus QUIC-v2 OS follow-up (reliability/recovery/congestion hardening) | TASK-0003, TASK-0020, TASK-0022, TASK-0023B | Draft |

---

## Queue (TASK-0015+)

Continue in numerical order after TASK-0014.

Notable upcoming tasks:
- **TASK-0019–0024 (+0023B)**: Security and DSoftBus follow-ons (ABI guardrails, streams/QUIC, refactor-before-feature expansion)
- **TASK-0018**: crashdump v1 closed with final hardening (identity/report fail-closed checks, deterministic marker proofs)
- **TASK-0019**: ABI syscall guardrails v2 completed (proof gates green; closed as done)
- **TASK-0016B**: completed networking-structure closure slice for `netstackd` (supports follow-on networking tasks)
- **TASK-0025–0028**: StateFS hardening + ABI filters v2
- **TASK-0029**: Supply chain v1 (SBOM + signing policy)
- **TASK-0031**: Zero-copy VMOs v1 (enables driver + graphics tracks)
- **TASK-0039**: Sandboxing v1
- **TASK-0054+**: UI stack
- **TASK-0054B / 0054C / 0054D**: early kernel/UI perf floor (zero-copy bulk stance + IPC fastpath + MM reuse)
- **TASK-0055B / 0055C / 0055D / 0056B / 0076B**: early visible UI path (scanout -> visible present -> dev presets/shell modes -> visible input -> visible DSL mount)
- **TASK-0056C**: present/input perf polish (click-to-frame latency + coalescing + skip paths)
- **TASK-0060B / 0062B**: glass compositor + animation perf scenes / fluidity gates
- **TASK-0080B / 0080C**: bootstrap SystemUI DSL shell + real launcher before the broader SystemUI migration
- **TASK-0067B / 0100B / 0122B / 0122C**: visible clipboard/audio surfaces + shared DSL app platform/integration kit

---

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | Blocked by |
|-------|---------|------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU device-class services | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers, offload, netdevd | TASK-0003, TASK-0010, TASK-0012B |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | UI tasks (0054+) |
| TRACK-NEXUSINFER-SDK | On-device ML runtime (CPU ref + future NPU), hybrid IPC | TASK-0031, TASK-0010, TASK-0280 |
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
