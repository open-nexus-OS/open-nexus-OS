---
schema: 1
title: "NEURON kernel"
role: guide
subsystem: kernel
sources: [source/kernel/neuron/src, source/kernel/neuron-boot/src]
contracts: [RFC-0005, RFC-0021, RFC-0085, RFC-0098, RFC-0100, ADR-0049, ADR-0073, ADR-0077, ADR-0080, ADR-0082, ADR-0091]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NEURON kernel

> **Purpose** — what the microkernel owns, where each part lives, and how a syscall, a task and a page move through it.
> **Owns** — `source/kernel/neuron/`, `source/kernel/neuron-boot/`
> **Contracts** — [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md), [RFC-0021](../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md), [RFC-0085](../rfcs/RFC-0085-kernel-owned-va-allocation.md), [RFC-0098](../rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md), [RFC-0100](../rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md)
> **Invariants** — the kernel holds only scheduling, memory, IPC and capabilities; drivers and policy live in userspace. The device tree is the only hardware truth. Syscall numbers are never reused.
> **Read next** — [smp.md](smp.md), [../storage/vmo.md](../storage/vmo.md), [../security/capabilities.md](../security/capabilities.md), [../boot-init/boot.md](../boot-init/boot.md)

## What the kernel owns

| area | owns | code |
|---|---|---|
| scheduling | per-CPU run queues, QoS classes, timer preemption, affinity | `source/kernel/neuron/src/sched/`, `source/kernel/neuron/src/core/smp/`, `source/kernel/neuron/src/core/cpu_main.rs` |
| memory | frame pool, VMOs, address spaces, page tables, VA placement | `source/kernel/neuron/src/mm/`, `source/kernel/neuron/src/va_space.rs` |
| IPC | endpoints, message queues, budgets, EOF | `source/kernel/neuron/src/ipc/` |
| capabilities | per-task tables, rights, transfer | `source/kernel/neuron/src/cap/` |
| tasks | task table, exit reasons, reaping, process orders | `source/kernel/neuron/src/task/` |
| sync objects | timers, waitsets, fences | `source/kernel/neuron/src/timer.rs`, `source/kernel/neuron/src/waitset.rs`, `source/kernel/neuron/src/fence.rs` |
| platform | device-tree parse, PLIC, console, timer source | `source/kernel/neuron/src/hal/`, `source/kernel/neuron/src/core/boot_fdt.rs` |

## Boot in one paragraph

`source/kernel/neuron-boot/src/main.rs` (`_start`) relocates the static PIE, builds the platform
from the device tree in `a1`, switches to the Sv39 high half and builds the frame pool from the
tree's memory banks. `source/kernel/neuron/src/core/kmain.rs` then creates the
scheduler, capability tables, IPC router and syscall table, runs the in-kernel selftests
(`source/kernel/neuron/src/selftest/`), starts the secondary harts over SBI HSM and spawns init.
Full chain: [boot.md](../boot-init/boot.md).

## How a syscall enters

1. `ecall` traps into `source/kernel/neuron/src/arch/riscv/trap.S`, which saves the frame on the
   hart's trap stack (through `sscratch`).
2. `__trap_rust` in `source/kernel/neuron/src/core/trap/handler.rs` dispatches by cause: S_SOFT
   (IPI), S_TIMER (tick, preemption), S_EXT (PLIC), page fault, or `ecall` → `handle_ecall`.
3. `handle_ecall` reads the number from `a7`, arguments from `a0..a5`; a pending order from
   another hart (kill, freeze; ADR-0082) runs first.
4. Three lock classes (ADR-0049):
   - lock-free — only `nsec` (`lockfree_syscall`) runs before any lock;
   - phased — `vmo_create`, `exec_vmo`, `vm_unmap`, `vmo_clean` run their expensive middle with the
     big kernel lock dropped (`source/kernel/neuron/src/core/trap/phased.rs`);
   - everything else runs under the BKL (`KernelGuard`).
5. `SyscallTable::dispatch` (`source/kernel/neuron/src/syscall/mod.rs`) calls the handler in
   `source/kernel/neuron/src/syscall/api/`; the window is 128 numbers.
6. The result goes back in `a0`: a value, or `-errno` (`source/kernel/neuron/src/core/trap/errno.rs`).
   Map failures keep their identity (ADR-0054): `EEXIST` occupied, `EFAULT` non-canonical,
   `ENOMEM` no hole, `ENOENT` nothing mapped, `EPERM` rights or W^X.

## Syscalls

The authoritative list is the `SYSCALL_*` constants in `source/kernel/neuron/src/syscall/mod.rs`;
retired numbers stay retired.

<!-- gen:syscalls -->
| # | syscall | what |
|---|---|---|
| 0 | `yield` | Gives up the CPU to the next runnable task. |
| 1 | `nsec` |  |
| 2 | `send` |  |
| 3 | `recv` |  |
| 5 | `vmo_create` | Creates a VMO. |
| 6 | `vmo_write` |  |
| 7 | `spawn` | Starts a thread in the caller's OWN address space (`as_self`), suspended (ADR-0091): a task never names anot… |
| 8 | `cap_transfer` |  |
| 11 | `exit` |  |
| 12 | `wait` |  |
| 14 | `ipc_send_v1` | IPC v1 (payload copy-in): see RFC-0005. |
| 15 | `task_qos` | Task QoS ABI (get/set self + privileged set-for-target). |
| 16 | `debug_putc` | Debug UART putc for userspace (best-effort, no permissions required). |
| 18 | `ipc_recv_v1` | IPC v1 (payload copy-out): see RFC-0005. |
| 19 | `ipc_endpoint_create` | Create a new kernel IPC endpoint and return a capability slot for it (privileged; RFC-0005). |
| 20 | `cap_close` | Drops a capability slot; if the capability has `Rights::MANAGE` and is an endpoint, closes it. |
| 21 | `ipc_endpoint_close` | Closes a kernel IPC endpoint referenced by a capability slot with `Rights::MANAGE`. |
| 22 | `ipc_endpoint_create_v2` | Creates a new IPC endpoint using an explicit endpoint-factory capability (Phase-2 hardening). |
| 23 | `ipc_endpoint_create_for` | Creates a new IPC endpoint on behalf of `owner_pid` (init-lite bootstrap helper). |
| 24 | `cap_clone` | Clones a capability slot locally (creates a second reference in the caller). |
| 25 | `getpid` | Returns the current task PID. |
| 26 | `ipc_recv_v2` | Receives an IPC message and additionally returns sender service identity metadata (v2). |
| 28 | `cap_query` | Queries a capability slot and writes (kind_tag, base, len) to a user buffer. |
| 29 | `spawn_last_error` | Returns the last spawn failure reason for the current task (RFC-0013). |
| 30 | `device_cap_create` | Creates a DeviceMmio capability in the caller's cap table (privileged; init-only). |
| 31 | `cap_transfer_to` | Transfers a capability into a specific slot in the child task. |
| 32 | `task_resume` | Resume a suspended task (enqueue into scheduler). |
| 33 | `timer_create` | Creates a kernel timer object and returns a timer capability slot. |
| 34 | `timer_set` | Arms a timer capability with an absolute monotonic deadline. |
| 35 | `timer_cancel` | Cancels a previously armed timer capability. |
| 36 | `irq_bind` | Binds an external interrupt (PLIC source) to an endpoint capability so the kernel delivers that device IRQ t… |
| 37 | `irq_complete` | Acknowledges a delivered IRQ so the PLIC can re-arm it. |
| 38 | `waitset_create` | Creates an empty waitset capability (RFC-0033). |
| 39 | `waitset_add` | Adds an endpoint (RECV right required) as a member of a waitset. |
| 40 | `waitset_wait` | Blocks until any member endpoint of a waitset has a pending message, then returns the ready member index; `-… |
| 41 | `fence_create` | Creates a timeline fence capability (value starts at 0). |
| 42 | `fence_signal` | Advances a fence monotonically to at least `value`, waking satisfied waiters. |
| 43 | `fence_wait` | Blocks until the fence value reaches `target`; `-ETIMEDOUT` on deadline. |
| 44 | `debug_write` | Debug UART slice write: emits a whole byte slice atomically under the UART lock so a userspace log line cann… |
| 45 | `boot_mode` | Boot mode query: returns 1 when the kernel resolved an INTERACTIVE boot (verdict folding on), 0 for proof/un… |
| 46 | `vmo_destroy` | Releases a sole-owned VMO back to the kernel arena (task #124). |
| 47 | `vmo_read` | Reads bytes out of a VMO into a caller buffer (mirror of `SYSCALL_VMO_WRITE`). |
| 48 | `sched` | B (TASK-0042): multiplexed scheduling-attribute syscall (affinity + shares). |
| 49 | `as_self` | C (Phase C): returns the caller's own address-space handle (raw) so a task can spawn threads into its OWN ad… |
| 51 | `vmo_share_ro` | RFC-0080: derive a READ-ONLY alias (`VmoRo`) of a VMO for shared read-only mapping (the glyph atlas). |
| 52 | `wait_nohang` | RFC-0081: non-blocking reap of ONE ready zombie child. |
| 53 | `vm_map` | RFC-0085: maps a whole VMO range at a KERNEL-CHOSEN va inside the managed user window and returns that va. |
| 54 | `vm_unmap` | RFC-0085: unmaps ONE exact region previously returned by `vm_map`/ `mmio_map_auto` (whole region, v1 has no… |
| 55 | `mmio_map_auto` | RFC-0085: maps a device-MMIO window at a kernel-chosen va (USER\|RW floor, never EXEC). |
| 56 | `system_reset` | TASK-0050 (ADR-0055): SBI SRST system reset. |
| 57 | `boot_handoff` | TASK-0289 B1 (ADR-0059): copies the loader's validated measured-boot record (raw `bootfmt::handoff` bytes, 6… |
| 58 | `ipc_call_v1` | TASK-0054C P4a (RFC-0096, ADR-0064): request + reply in ONE trap. |
| 59 | `ipc_reply_recv_v1` | TASK-0054C P4b: reply + wait for the next request in ONE trap. |
| 60 | `vmo_runs` | TASK-0286 P4a (RFC-0098 C4): the one door a physical address leaves the kernel by, device-scoped since TASK-… |
| 61 | `mm_stats` | TASK-0286 P5 (RFC-0098 C4): the memory record — the pool, the objects and the caller's own residency (`accou… |
| 62 | `child_exit_bind` | Binds the caller's child-death endpoint (TASK-0333 6a): each child's exit writes one `OP_CHILD_EXITED` frame… |
| 63 | `exec_vmo` | ADR-0080: makes a process from a SEALED image object — the one loader. |
| 64 | `vmo_seal` | ADR-0080: seals the VMO in a slot — no writer for the rest of its life. |
| 65 | `task_suspend` | ADR-0082 (RFC-0111): freezes a CHILD process — the child and every thread of it; a task running on another h… |
| 66 | `task_kill` | ADR-0082 (RFC-0111): kills a CHILD process the parent marked `killable` at `exec_vmo` (`ExitReason::Killed`,… |
| 67 | `mem_pressure_bind` | ADR-0085 (RFC-0100 §3, TASK-0333 6e): binds the caller's endpoint as the kernel's ONE memory pressure sink —… |
| 68 | `vmo_purgeable` | RFC-0100 §4 (TASK-0333 6e-3): unlocks (arg 1 = 1) a lazy VMO for purging — the kernel may drop its pages at… |
| 69 | `task_squeeze` | ADR-0087 (RFC-0100 §5, TASK-0333 6f-1): squeezes one bounded batch of a parked CHILD's private lazy pages —… |
| 70 | `swap_key` | ADR-0088 (RFC-0100 §5, TASK-0333 6f-2): seeds the boot's swap key — once, from the RNG service's kernel iden… |
| 71 | `swap_export` | ADR-0088: seals one bounded batch of a process's records into the pager's buffer (header, entries `token, ct… |
| 72 | `swap_commit` | ADR-0088: the pager wrote the record of `token` sealed under `ctr` to `slot` — the page is swapped, its reco… |
| 73 | `swap_page_in` | ADR-0088: the pager's answer to `OP_PAGE_IN` (or its prefetch): the sealed record of `token` from `slot`, op… |
| 74 | `vmo_clean` | ADR-0092 (ADR-0079's deferred call): clean the CPU caches over `[offset, offset + len)` of an object the cal… |
<!-- /gen:syscalls -->

## Scheduling

- **Preemptive.** The `timer_irq` feature is on by default; the tick is 10 ms of the device tree's
  timebase. Per-hart deadlines arm the compare register earliest-first (ADR-0052).
- **SMP.** Up to `MAX_CPUS` = 4 harts. Every hart runs `cpu_main` on its own run-queue set;
  cross-CPU operations (wake to home CPU, bounded steal) are serialized by the BKL. Lane defaults
  and details: [smp.md](smp.md).
- **QoS.** Four classes per CPU queue — `Idle`, `Normal`, `Interactive`, `PerfBurst` — round-robin
  within a class, a deterministic reject when a queue is full.
- **A wake preempts (ADR-0096).** A wake of a class above the running task's switches to it at
  the trap exit — after an interrupt, a timer or a syscall that woke it
  (`Scheduler::ready_above_current`). init raises the soft-realtime chain
  (`service_topology::soft_realtime`) to `Interactive` before its wake; everything else runs
  `Normal` and may only lower itself.
- **Placement.** init pins the soft-realtime chain to cpu0 and background services to cpu1–3 at
  resume (`affinity_for`, ADR-0049); `sched` carries affinity and shares.
- **No lock across a context switch.** The schedule decision copies the next frame into the
  hart-local staging slot under the BKL, the guard drops, then `sret` reads only hart-local state.

## Memory

- **Frames.** A buddy allocator per memory bank (`source/kernel/neuron/src/mm/frames/`), one live
  pool (`source/kernel/neuron/src/mm/frame_pool.rs`), reached through one direct map
  (`source/kernel/neuron/src/mm/phys.rs`); no fixed physical window exists (`just fixed-windows`).
- **VMOs.** A VMO is a list of pool blocks; capabilities name the object, never the frames. Kinds:
  anonymous, contiguous (only for a device holder), lazy. An object dies with its last capability
  and mapping (RFC-0100); `vmo_seal` makes it read-only for life; `vmo_share_readonly` mints a
  read-only alias. Userspace view: [vmo.md](../storage/vmo.md).
- **Commit on touch** (ADR-0077). A lazy VMO takes no frame at create; a page takes one zeroed
  frame at its first touch.
- **Kernel-chosen addresses** (RFC-0085). `vm_map` and `mmio_map_auto` place a mapping inside the
  managed window — from `USER_VM_WINDOW_BASE` to `USER_VADDR_LIMIT`, the whole user half
  (`source/kernel/neuron/src/mm/mod.rs`) and return the address.
  Placement policy is host-tested in `source/kernel/neuron/src/va_space.rs`.
- **Tables grow with their use** (ADR-0093). The kernel heap (`source/kernel/neuron/src/mm/kheap.rs`),
  lazy page maps, regions, capabilities and endpoints grow from the pool, never bounded by a
  constant; what the pool cannot give answers `ENOMEM`, never a panic.
- **W^X.** The page table refuses a writable, executable leaf; `vm_map` refuses EXECUTE.
- **DMA.** A device capability carries its DMA reach; `vmo_runs` is the only way a bus address
  leaves the kernel (`source/kernel/neuron/src/mm/dma_runs.rs`, `source/kernel/neuron/src/mm/dma_reach.rs`).
- **Pressure** (ADR-0085). The pool reports a watermark level to one sink; memd decides
  (ADR-0086–0088).

## Process model

- **One loader.** `exec_vmo` makes a process from a sealed image object (ADR-0080): text and
  read-only data mapped from the image's frames, data copied, `.bss` and stack lazy.
- **Own space only** (ADR-0091). `spawn` starts a thread in the caller's own space, suspended for
  its parent; any other space is `EPERM`, and no task maps into another's.
- **A process is its address space** (ADR-0082). The parent orders it whole — `task_resume`,
  `task_suspend`, `task_kill` — and a leader's exit takes its threads.
- **Death is an event** (ADR-0073). Every kernel object dies with its last capability; a child's
  exit lands in an endpoint the parent bound (`child_exit_bind`); execd reaps with `wait_nohang`
  (RFC-0081). Release path: `source/kernel/neuron/src/task/reap.rs`.

## Capabilities

- Per-task table (`CapTable`, `source/kernel/neuron/src/cap/mod.rs`). Kinds: endpoint, endpoint
  factory, VMO, read-only VMO alias, device MMIO (window + PLIC line + DMA reach), IRQ, timer,
  waitset, fence, affinity control.
- Rights: `SEND`, `RECV`, `MAP`, `MANAGE`, `WAIT`. A transfer or clone intersects rights with the
  source's; they never grow.
- Identity is the kernel's: `ipc_recv_v2` reports the sender's service id; a payload string never
  is.
- Device capabilities are minted by init from the device tree; MMIO maps are USER|RW, never EXEC.
- Holders: [capabilities.md](../security/capabilities.md).

## Platform from the device tree (RFC-0098)

Nothing in the kernel names an address, interrupt line or frequency of its own. `hal/platform.rs`
fills console, PLIC, timebase and timer source (Sstc or SBI) once from the tree. init gets the tree
read-only in a fixed slot and mints device capabilities from it. `scripts/check-no-platform-literals.sh`
(in `just check`) fails on a QEMU-virt literal outside the tree's goldens.

## Diagnostics and tests

| need | where |
|---|---|
| host unit tests | `just test-kernel` (`cargo test -p neuron`) |
| warnings gate | `just diag-kernel` |
| in-kernel selftests (`KSELFTEST: …`) | `source/kernel/neuron/src/selftest/` |
| trap diagnostics, fault paths | `source/kernel/neuron/src/core/trap/fault.rs`, `source/kernel/neuron/src/core/trap/illegal.rs` |
| BKL budgets and contention | `source/kernel/neuron/src/core/trap/budgets.rs`, `source/kernel/neuron/src/core/trap/lock_stats.rs` |
| logging, UART, liveness | `source/kernel/neuron/src/diag/` |
| debug-only mutex | `source/kernel/neuron/src/sync/dbg_mutex.rs` |

Opt-in features (`source/kernel/neuron/Cargo.toml`): `debug_uart`, `trap_symbols`, `trap_ring`,
`ipc_trace_ring`, `selftest_ipc`, `selftest_caps`, `selftest_sched`, `debug_pt_verify`. QEMU proof:
`just test-os` and [../testing/README.md](../testing/README.md).
