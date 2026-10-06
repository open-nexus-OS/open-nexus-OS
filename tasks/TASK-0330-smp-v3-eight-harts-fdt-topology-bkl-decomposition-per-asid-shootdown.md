---
title: TASK-0330 SMP v3: n harts from the FDT (boot-sized per-hart state, 64-bit CpuSet ABI, MAX_CPUS deleted) + cluster topology, per-hart PLIC/IRQ affinity, the IPC router and the address spaces out of the BKL, per-ASID shootdown
status: Draft (recut 2026-10-06 to the end state — n harts from the FDT, not `MAX_CPUS` 4 → 8; no distributed kernel; seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; starts after Block 3)
owner: @kernel-team
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Lock classes and budgets: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md, source/kernel/neuron/src/core/trap/budgets.rs
  - Per-hart EDT: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
  - Scheduler half: tasks/TASK-0306-ui-responsiveness-input-integrity-smp-rendezvous.md (Phase 3)
  - IPC fastpath numbers to hold: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md
  - Gate consumer: tasks/TRACK-TIME-AS-RESOURCE.md
  - RFC/ADR seeds (at P0): RFC-0102 SMP lock decomposition, ADR-0072 cluster-aware spreading, ADR-0073 BKL split order (from R9)
---

## Origin

No ledger carried the remaining SMP work: 0012/0012B/0042/0247/0277/0283/0288 are Done, `TASK-0306` Phase 3 (BKL hold) is the scheduler half and stays its own ledger (S2). Minted for target picture S; B/C parts seeded at P0.

## Context

`MAX_CPUS = 4`, per-hart runqueues live inside ONE big kernel lock (`core/trap/runtime.rs`) covering scheduler, tasks, IPC router, address spaces, timers, waitsets and fences; TLB shootdown is a full flush on every hart; PLIC delivery is boot-hart only; the display chain is pinned to cpu0. The board has 8 harts in two clusters.

## Goal

**End state, not a bigger constant** (operator decision 2026-10-06): the hart count comes from
the FDT at boot; every per-hart structure is allocated from a boot slab sized by it; the only
compile-time ceiling is the CPU-set width (`CpuSet` = 64 bits, an ABI — RFC-0107 — replacing the
`u8` affinity mask, the real limit of 8 today); `MAX_CPUS` is deleted with a retired-name gate
(`CPU_MASK_BITS` stays). The cluster topology (`cpu-map`: the board's 2 × 4 harts) is a kernel
model exported to init/execd — no literal masks anywhere in userspace. S1 (this ledger): the
above + PLIC contexts and IRQ affinity on every hart (`KSELFTEST: smp online ok harts=8
clusters=2`, IRQs served off hart 0) + the HSM retry bug fixed (`retry_missing_harts` passes a
virtual start address, `bringup.rs:273`, while the first start uses `virt_to_phys` — the likely
MTTCG "lost hart" flake behind `[profile.smp] SMP="2"`). S3 (`0330B`): IPC router + waitsets out
of the BKL. S4 (`0330C`): address spaces out of the BKL, per-ASID targeted shootdown. S6 (new,
small): the compute-distribution seam — ADR-0074 "jobs travel, capabilities never; one kernel per
machine; brokers compose", pinched `Backend::{Local, Remote}` with `Remote` refused by name, the
job's determinism contract host-tested. **Not a multikernel**: replicating kernel state per core
buys nothing on a coherent 8-hart SoC and costs every cap/task/VMO twice; the scaling the
multikernel principle promises comes from the lock decomposition (S2–S4), and distribution
across machines is a userspace concern (RFC-0028: no remote execution, no capability transfer).

### Measured inventory (S0, 2026-10-06 — what `MAX_CPUS` sizes today; `source/kernel/neuron/src/`)

| Static | Where | Boot-sized in S1 |
|---|---|---|
| `HART_LOCALS` (a `TrapFrame` each, fixed address for `trap.S`, `tp` range check) | `core/smp/mod.rs:95` | stays static at `CPU_MASK_BITS` (≈32 KiB) |
| secondary stacks 3 × 64 KiB in .bss | `core/smp/bringup.rs:42` | per started hart from the frame pool |
| resched/IPI counters, wake hints, timer ticks, steal gate | `core/smp/mod.rs:104-123`, `runtime.rs:30-85` | slab |
| IRQ stash `[[AtomicU64; 16]; N]` | `core/irq.rs:99` | slab (the boot hart's entry before the first trap) |
| PLIC S-contexts | `hal/platform.rs:50` | slab |
| deadline shadow (ADR-0052) | `core/trap/runtime.rs:357` | slab |
| console line buffers | `hal/console_line.rs:52` | slab (boot hart's entry early) |
| TLB mailboxes + activity | `core/smp/tlb.rs:46,70` | slab |
| `Scheduler.cpus`, `TaskTable.current` | `sched/mod.rs:145`, `task/mod.rs:504` | `Vec` from `hart_count()` |
| `affinity_mask: u8`, `ALL_CPUS_MASK` | `task/mod.rs:32,288`, `task/affinity.rs` | `CpuSet(u64)` |
| literal masks `0b0001/0b1111/0b1110`, `0xF`, `MAX_WORKERS = 4`, `1 << (idx & 7)`, `PINCHED_WORKERS = 2` | `init/affinity.rs`, `bootstrap/resume.rs:32`, `execd/sched_recipe.rs:18`, selftest `soaks.rs:21`, `nexus-workpool/pool.rs:29,182`, pinched | from the exported topology; never a worker on the soft-RT hart |

Bring-up today: `start_secondary_harts` loops `1..MAX_CPUS`; the board's harts 4–7 (the whole
second cluster) stay in HSM STOPPED unnamed. Boot hart ≠ 0 is a silent assumption — named as a
FAIL in S1.

## Non-Goals

The scheduler lock itself (`TASK-0306` Phase 3 = S2), the placement policy (`TASK-0042B` = S5), heterogeneous scheduling beyond cluster awareness.

## Packages (from the order file; the P0 rewrite fixes them)

- **S0** — measure: the inventory above; `-smp 8` MTTCG ×10 boots against the retry bug (H1); R9 BKL hold histogram + IPC rate per hart on the board at 4 harts (the baseline the split order follows); the `cpu-map`.
- **S1** (0330) — n harts from the FDT (boot slab, `CpuSet` ABI, `MAX_CPUS` deleted), cluster topology exported, per-hart PLIC contexts, IRQ affinity, retry fix. Gates: host tests (topology vs `bpi-f3.dtb`/`virt.dtb` goldens, `CpuSet` rejects, pin rejects), `ci-os-smp` (2 harts) green, `smp8` best-effort until H1 holds ×10, board `KSELFTEST: smp online ok harts=8 clusters=2` + IRQ-off-boot-hart marker, `board-headless`/`-visible` green, retired-name gate.
- **S3** (0330B, at P0) — IPC router + waitsets off the BKL. Gate: RFC-0096 numbers per hart; time-as-resource gate green.
- **S4** (0330C, at P0) — address spaces off the BKL + per-ASID shootdown. Gate: shootdown-count gate (targeted, not full).
- **S6** (small) — the compute-distribution seam: ADR-0074, pinched `Backend::Remote` declared and refused by name (`test_reject_remote_backend_until_built`), the job's determinism contract over both backends host-tested. No network code; dsoftbus discovery (TASK-0331) is a later dependency.
- **Not in this lane** (operator 2026-10-06: the lane must not grow): the first parallel consumer — the band-parallel CPU present (gpud's executor on the workpool; 30 ms full frame at 1080p, the typing flicker, xhcid starved meanwhile) — rides on S5 under G3/G4 / TASK-0251 finding 6.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: S0's measurements (R9, the `-smp 8` boots, the inventory) are done BEFORE any kernel edit; the split order (ADR-0073) is written from the histogram, not from intuition.
- **RED**: `HART_LOCALS` at the mask width changes `trap.S` layout constants — released only with the `cpu_current_id` fallback counter at 0 on QEMU and the board.
- **YELLOW**: the boot slab must exist before the boot hart's first trap uses the IRQ stash / console line (the boot hart's entry stays static or the slab precedes the first trap).
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
