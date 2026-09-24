# NEURON Microkernel (v0 Increment 1)

The first NEURON milestone focuses on a minimal, well documented
microkernel capable of booting on the QEMU RISC-V `virt` machine,
printing a banner over UART and exposing a deterministic syscall
surface for early user tasks.

## Security north star (system-level)

NEURON is the kernel core of a capability-service-oriented, Rust-first, RISC‑V-first system.
The system security roadmap is intentionally hybrid:

- Verified boot + signed bundles/packages + capability-based isolation as the MVP root.
- Pluggable key custody via `keystored`/`identityd` that can later use Secure Element / TEE
  without rewriting kernel interfaces.
- Measured boot/attestation later for distributed trust (`softbusd`) without inflating kernel TCB.

## Debug / Diagnostics Index (kernel)

This section is a navigation index for deterministic bring-up and kernel debugging.
It is **documentation-only** and must stay compatible with the QEMU marker contract
implemented by `scripts/qemu-test.sh` (marker strings are a gating surface).

### Feature flags (selected)

- **`debug_uart`**: Enables additional UART prints in selected paths. Must remain bounded.
- **`trap_symbols`**: Adds opt-in trap symbolization (`name+offset`) for `sepc` in traps.
- **`trap_ring`**: Retains a bounded ring of recent trap frames for post-mortem.
- **`timer_irq`**: Arms periodic timer IRQ ticks (bring-up typically keeps this off by default).
- **`selftest_priv_stack`**: Runs kernel selftests on a private guarded stack.
- **`selftest_time` / `selftest_ipc` / `selftest_caps` / `selftest_sched`**: Incremental selftest coverage gates.

### Marker families (where they originate)

- **Boot/bring-up**: emitted by boot + kmain bring-up logic (e.g. banner / mapping markers).
- **Kernel selftests**: emitted by `selftest` (`KSELFTEST: ...`).
- **Userspace/selftest client**: emitted by the OS smoke harness and userspace services (`SELFTEST: ...`).
- **Panic/trap diagnostics**: `PANIC:` / `EXC:` style lines are for negative-path debugging and should remain deterministic.

### Where to look (current paths)

- **Boot init**: `source/kernel/neuron/src/core/boot.rs`
- **Kernel bring-up / state**: `source/kernel/neuron/src/core/kmain.rs`
- **Trap handling / trap diagnostics**: `source/kernel/neuron/src/core/trap.rs`
- **SATP switch island marker**: `source/kernel/neuron/src/mm/satp.rs`
- **Task lifecycle (spawn/exit/wait)**: `source/kernel/neuron/src/task/mod.rs`
- **Bootstrap protocol layout**: `source/kernel/neuron/src/task/bootstrap.rs`
- **Structured logging**: `source/kernel/neuron/src/diag/log.rs`
- **UART writer / raw UART**: `source/kernel/neuron/src/diag/uart.rs`
- **Determinism knobs**: `source/kernel/neuron/src/diag/determinism.rs`
- **Bring-up watchdog**: `source/kernel/neuron/src/diag/liveness.rs`
- **Debug-only sync**: `source/kernel/neuron/src/diag/sync/dbg_mutex.rs`
- **In-kernel selftests**: `source/kernel/neuron/src/selftest/mod.rs`

## Boot Flow

1. `_start` (`neuron-boot/src/main.rs`) applies the image's own
   `R_RISCV_RELATIVE` table — the kernel is a static PIE linked at 0 and runs
   wherever the previous stage loaded it (nxboot's window, or the firmware
   entry on a direct `-kernel` boot) — sets `sp`/`gp` and enters
   `early_boot_init(hartid, dtb)`: BSS is zeroed, the firmware registers are
   recorded (`core/boot_fdt.rs`) and the platform is built from the device tree
   (`hal/platform.rs`) before the first log line. The kernel then moves to the
   Sv39 high half (RFC-0098 C4, TASK-0286 P2): `early_boot_init` returns the
   `satp` of a boot table of 1 GiB leaves (every bank, the two device windows,
   the tree, plus the identity of the gigabyte the switch runs in); `_start`
   writes it, jumps to `PHYS_OFFSET + PC`, re-applies the fixups with the high
   base, and enters `high_boot_init` (traps, timer, heap — nothing holding a
   pointer is built before the switch; `KINIT: kernel high half (base=… load=…)`).
   Secondary harts run the same switch in their stub, off the published boot
   `satp`, before they touch `sp` or Rust. `high_boot_init` also builds the
   frame pool (`mm/frame_pool.rs`, over `crate::frames`) from the tree's banks
   minus reserved ranges, the image and the tree (`KINIT: mm frames (…)`);
   every page table, VMO, process image, spawn stack and init page is a frame
   from it — no fixed physical window exists (gate `fixed-windows`).
2. `kmain` activates the kernel address space (the image at its high alias,
   every memory bank through the direct map, the UART/PLIC windows and the
   tree when it lies outside a bank), resolves boot mode + display
   request from `/chosen/nexus,*` (`diag/boot_mode.rs`), prints the platform,
   image and handoff markers, instantiates scheduler, capability table, IPC
   router and syscall table, then brings up SMP state and the secondary harts
   via SBI HSM.
3. The `NEURON` banner is emitted on the console the tree named, proving that
   the platform read is right before any driver exists.

## Platform from the device tree (RFC-0098)

The device tree handed over in `a1` is the ONE hardware truth; nothing in the
kernel names an address, an interrupt line or a frequency of its own.

- **`hal/platform.rs`** — lock-free statics filled once by `init_from_fdt`,
  paging off: the console from `/chosen/stdout-path` (`reg`, `reg-shift`,
  `reg-io-width` — the byte-stride `ns16550a` and a 4-byte-stride part are one
  driver), the PLIC base + `riscv,ndev` + the S-mode context of every hart from
  `interrupts-extended`, the timebase from `/cpus`, the timer source from the
  boot hart's ISA (`stimecmp` when Sstc is listed, SBI `set_timer` otherwise;
  CLINT MMIO is never touched from S-mode). Tick↔ns conversions are exact
  64-bit factors derived from the timebase. Markers:
  `KSELFTEST: platform from fdt ok (uart=… plic=… ndev=… tb=…Hz harts=… timer=…)`,
  `KINIT: timer sstc|sbi`.
- **`core/boot_fdt.rs`** — records `a1` before paging, maps the tree's pages
  read-only, exposes it to the kernel (`bytes()`) and injects a read-only
  alias (`VmoRo`) into init's slot 2 (`nexus_abi::INIT_DEVICE_TREE_SLOT`),
  from which init discovers devices and hands the tree to services.
- **`core/boot_image.rs`** — the image's own range and the fixup-table check:
  `KSELFTEST: kernel image ok (base=… len=… relocs=…)`.
- **`core/boot_handoff.rs`** — the measured-boot record from
  `/chosen/nexus,boot-record` (ADR-0059 v1 bytes, carried by nxboot).
- **Device capabilities carry their line** — `DeviceMmio { base, len, irq }`
  is minted by init from a node's `reg` + `interrupts`; `cap_query` reports the
  line, and `irq_bind`/`irq_complete` accept only a line the caller's device
  capability carries.
- **Gate** — `scripts/check-no-platform-literals.sh` (in `just check`) fails on
  any QEMU-virt address, slot-index interrupt arithmetic or timebase constant
  outside the tree, its goldens and tests.

## Syscall Surface

The syscall surface is intentionally small but evolves during bring-up.
The authoritative list (including numeric IDs) lives in `source/kernel/neuron/src/syscall/mod.rs`.

- **0 `yield`**: Rotate the scheduler and return the next runnable task id. Activates the target task's address space.
- **1 `nsec`**: Return the monotonic time in nanoseconds derived from the `time` CSR.
- **2 `send`**: Send an IPC message via an endpoint capability.
- **3 `recv`**: Receive the next pending IPC message.
- **4** — RETIRED (RFC‑0085 P6): was the fixed‑VA `map`; number never reused. Use 53 `vm_map`.
- **5 `vmo_create`**: Create a page-backed VMO (`mm::vmo`, TASK-0286 P3a): a list of physically contiguous blocks from the frame pool; arg 2 bit 0 asks for ONE block (`nexus_abi::vmo_create_contiguous`) — for memory a device addresses by one base (virtio queues, command pools); only drivers create it (`just dma-contiguous`).
- **6 `vmo_write`**: Write bytes into a VMO capability.
- **7 `spawn`**: Create a child task (fresh Sv39 AS by default) with a guarded stack.
- **8 `cap_transfer`**: Duplicate/grant a capability to another task with a rights mask (subset-only).
- **9 `as_create`**: Allocate a new Sv39 address space and return its opaque handle.
- **10 `as_map`**: Map a VMO into a *target* address space identified by handle. Enforces W^X at the syscall boundary.
- **11 `exit`**: Terminate the current task.
- **12 `wait`**: Wait for a child task exit.
- **13 `exec`**: Execute an ELF payload (loader path).
- **14 `ipc_send_v1`**: Kernel IPC v1 send (payload copy-in) (see RFC‑0005).
- **15 `task_qos`**: Scheduler QoS hint for the calling task.
- **16 `debug_putc`** / **44 `debug_write`**: Byte / bounded-buffer UART output.
- **17 `exec_v2`**: ELF exec with the v2 bootstrap-info page contract.
- **18 `ipc_recv_v1`**: Kernel IPC v1 recv (payload copy-out) (see RFC‑0005).
- **19 `ipc_endpoint_create`** / **21 `…_close`** / **22 `…_create_v2`** /
  **23 `…_create_for`**: Kernel IPC endpoint lifecycle (see RFC‑0005).
- **20 `cap_close`** / **24 `cap_clone`**: Capability slot lifecycle.
- **25 `getpid`**: Caller's task id.
- **26 `ipc_recv_v2`**: IPC recv with sender identity + cap-move (ADR‑0042 transport).
- **27** — RETIRED (RFC‑0085 P6): was the fixed‑VA `mmio_map`; number never reused. Use 55 `mmio_map_auto`.
- **28 `cap_query`**: Query a capability slot (kind/irq/base/len) into a user buffer; a device capability's `irq` is the PLIC line init took from the device tree (RFC‑0098 C3) — the one place a driver learns its interrupt. `base` is a device's register window; a VMO reports 0 (its physical runs leave only through 60 `vmo_runs`). 32 bytes since TASK‑0286 P4b: a device also reports `flags` (bit 0 = does not snoop the caches) and `cache_block` (the harts' Zicbom block, 0 = none).
- **29 `spawn_last_error`**: Last spawn-failure reason for the caller (RFC‑0013).
- **30 `device_cap_create`**: Mint a DeviceMmio capability (privileged bring-up): window, PLIC line and, in arg 4 bit 0, `dma-noncoherent` from the node or its bus (RFC‑0098 C4; any other bit is refused).
- **31 `cap_transfer_to`**: Transfer a capability into a specific child slot.
- **32 `task_resume`**: Resume a suspended task.
- **33–35 `timer_create/set/cancel`**: Per-task timer capabilities.
- **36 `irq_bind`** / **37 `irq_complete`**: PLIC IRQ → endpoint delivery (reactive input).
- **38–40 `waitset_create/add/wait`**: Bounded waitsets.
- **41–43 `fence_create/signal/wait`**: Fences.
- **45 `boot_mode`** / **50 `boot_display_mode`**: Boot mode and display request from `/chosen/nexus,*` of the device tree (RFC‑0098 C2; RFC‑0074/ADR‑0050 for the display request — nxboot re-expresses the QEMU fw_cfg knobs there, the kernel reads no fw_cfg).
- **46 `vmo_destroy`**: Return a VMO's frames to the pool (sole-owner and not-mapped gated; RFC‑0075/0085).
- **47 `vmo_read`**: Bounded copy-out of a VMO range (ADR‑0042 damage blits).
- **48 `sched`**: Declarative scheduling recipe (affinity/shares; ADR‑0049).
- **49 `as_self`**: The caller's own address-space handle.
- **51 `vmo_share_readonly`**: Mint a read-only VMO alias — WRITE|EXEC kernel-stripped (RFC‑0080).
- **52 `wait_nohang`**: Non-blocking child reap (RFC‑0081).
- **53 `vm_map`**: Whole-range VMO map at a KERNEL-CHOSEN va inside the
  managed user window `[0x5000_0000, 0x8000_0000)`; returns the va. ≥2 MiB
  ranges get a pa-congruent va so interiors promote to 2 MiB superpage
  leaves (RFC‑0085).
- **54 `vm_unmap`**: Unmap one exact `vm_map`/`mmio_map_auto` region; one
  TLB shootdown per call (RFC‑0085).
- **55 `mmio_map_auto`**: Device-MMIO window at a kernel-chosen va —
  same USER|RW/never-EXEC floor as the retired 27, but the caller cannot
  collide because it never picks an address (RFC‑0085).
- **60 `vmo_runs`**: The one door a physical address leaves the kernel by
  (RFC‑0098 C4, TASK‑0286 P4a): the `(pa, len)` runs behind a byte range of a
  writable VMO, adjacent runs merged, at most 256 per call, all or nothing —
  only to a task holding a device capability, never for a read-only alias.
  A driver takes a queue's one base from it (`nexus_abi::vmo_dma_base`) and a
  scatter-gather list for a device that reads one (gpud's resource backings,
  windowd's framebuffer). Authority + clipping: `mm/dma_runs.rs`, host-tested.
- **61 `mm_stats`**: The memory record (TASK‑0286 P5): pool, page-table frames, objects and
  their DMA bytes, and the caller's own residency, versioned (`nexus_abi::mm_stats`). No
  authority — nothing in it names another task; a buffer shorter than the record is refused.

Errors follow the conventional POSIX encoding: handlers return
`-errno` (two's complement) in `a0`. Key codes used by the current
increment:

- `EPERM` for capability or W^X violations.
- `EINVAL` for malformed arguments and IPC routing failures.
- `EEXIST` when a mapping is refused because the target VA is already
  occupied, and `EFAULT` for a non-canonical VA — map failures keep their
  identity across the ABI (ADR‑0054; no wildcard errno arms).
- `ENOSPC` when the ASID allocator is exhausted — and, since RFC‑0085,
  when the per-address-space region table is full.
- `ENOSYS` for disabled/unsupported functionality.
- `ENOMEM` when the guarded stack pool runs out of pages, or `vm_map`
  finds no hole of the requested size (`VM-MAP-FAIL reason=…` names the
  refusal in the log).
- `ENOENT` for `vm_unmap` of an address nothing is mapped at; `EBUSY` for
  `vmo_destroy` while any address space still maps the range (RFC‑0085).

> Current state note (2025-12-18): syscall handlers return `-errno` in `a0` for
> expected errors. The kernel may still terminate tasks in true “no forward
> progress” situations (e.g. repeated ECALL storms), but ordinary syscall errors
> are returned to userspace.

## Address Space Model

- Sv39 translation with three levels of page tables. Intermediate tables are allocated lazily as
  mappings are installed via `AddressSpaceManager::map_page`; every table page is an order-0
  frame from `mm::frame_pool`, zeroed on allocation and returned on drop (there is no static
  bring-up pool any more).
- The ASID allocator tracks 256 slots (ASID `0` is reserved for the kernel). Handles returned by
  `SYS_AS_CREATE` wrap the internal slot index and remain opaque to callers.
- Fresh address spaces are seeded with the kernel half (`mm/kernel_layout.rs`, RFC-0098 C4):
  the image at its high alias (`[__text_start..__text_end)` RX|GLOBAL, `[__text_end..__stack_bottom)`
  RW|GLOBAL, the kernel stack RW|GLOBAL above its guard page, left unmapped), every `/memory` bank
  through the direct map `PHYS_OFFSET + PA` (RW|GLOBAL, 2 MiB leaves, minus the image's own
  frames), the UART and PLIC windows, and the tree's pages when they lie outside every bank.
  The kernel half is built ONCE, for the kernel's own root; every later root adopts its entries
  256..512 (`adopt_kernel_half`), so a user address space costs its root plus its own user-level
  tables and the kernel's second-level tables are shared by all.
  Nothing kernel-owned lies below `KERNEL_VA_BASE`; page tables hold physical addresses and the
  kernel reaches the next level through `phys::phys_to_virt` — the one seam every physical
  dereference goes through (`vmo`, `exec`, the stack pool, the trap-time page walk, the console).
  GLOBAL keeps kernel pages visible across ASID switches.
- Kernel mapping finishes by emitting a single `map kernel segments ok` UART marker once the linker
  ranges have been installed. The SATP switch island performs an eight-byte RX-sanity sample around
  the current PC (panicking on all-zero fetch windows) before writing SATP, switches stacks inside
  the island page, and prints `AS: post-satp OK` after the TLB fence to prove the return
  path stayed within the island.
- Each address space maintains the set of owning tasks so the manager can reject destruction while
  references remain. Activating a handle writes SATP and issues a global `sfence.vma`.

## Physical Memory (RFC-0098 C4, TASK-0286 M1)

- **Where memory comes from.** Every `/memory` bank of the device tree minus `/reserved-memory`,
  the kernel image and the tree itself; nothing else. `mm/frames` is a buddy allocator per bank
  (4 KiB frames, blocks up to `MAX_ORDER` = 128 MiB, 2 MiB blocks superpage-aligned by
  construction; first-fit in bank order, deterministic), host-tested over both golden trees;
  `mm/frame_pool.rs` is the one live instance. No fixed physical window exists in the kernel
  (`just fixed-windows`); `-m` is a QEMU lane knob (`QEMU_MEM`, 256M and 1G proven).
- **How the kernel reaches it.** The kernel runs in the Sv39 high half behind a direct map,
  `KVA = PHYS_OFFSET (0xffff_ffc0_0000_0000) + PA`; `phys::phys_to_virt` / `virt_to_phys` are
  the one seam (see *Address Space Model*).
- **Objects.** A VMO (`mm/vmo.rs`) is a list of pool blocks: `Anon` (largest blocks first, what
  a device reads through a scatter-gather list or no device reads at all), `Contiguous` (one
  block, for a device that takes one base — virtio queues, command pools; only drivers create
  it, `just dma-contiguous`) or `Fixed` (frames outside the pool, never freed). Every object is
  zeroed at create, off the BKL. Capabilities name the object (`Vmo { id, len }`).
- **One door for a physical address.** `vmo_runs` (60) answers the runs of a byte range of a
  writable VMO to a task holding a device capability — never for a read-only alias; `cap_query`
  reports no VMO base (`KSELFTEST: vmo runs ok (runs=… deny=3)`).
- **Coherence.** A device that does not snoop the caches is marked `dma-noncoherent` on its node
  or bus; init mints that into the device capability, `cap_query` reports it with the harts'
  Zicbom block size, and the kernel enables `cbo.clean`/`cbo.flush` for user mode per hart
  (`senvcfg.CBCFE`, `CBIE = 01`: `cbo.inval` runs as a flush). Drivers maintain their buffers
  through `nexus_driverkit::DmaBuffer` over `nexus_abi::DmaVmo`
  (`SELFTEST: dma buffer ok (device=… block=… runs=…)`).
- **Accounting.** Read from the owners, never counted twice: `mm/usage.rs` collects the pool, the
  page-table frames, the objects (with their contiguous bytes) and each address space's
  residency (VMO and kernel-placed regions; device windows are not memory). `mm_stats` (61)
  answers the versioned record (`mm/accounting.rs`) with the caller's own residency — nothing
  about another task; `KSELFTEST: mm frames (…)` prints it at the ladder's late fence with the
  summary over all spaces (`spaces`, `rss_sum`, `rss_max`); metricsd records it as gauges at
  readiness (`metricsd: mm snapshot ok (…)`).
- **Exhaustion is an event** (RFC-0087 §1): only a request the pool cannot satisfy at all —
  an anonymous object's fallback to smaller blocks is the allocator's (`alloc_at_most`), not an
  exhaustion. The counter holds every one; the console line
  `MM: frames exhausted (…) event=exhaust.v1 resource=frames action=refused` appears on the
  1st, 2nd, 4th, 8th … occurrence.
- **Not yet.** Demand paging and CoW (M2), the page cache and shared read-only code (M3),
  pressure levels, quotas and the OOM handoff (M4, TASK-0287) — see TASK-0286's follow-ups.

## W^X Policy

- Writable and executable user mappings are mutually exclusive. `SYS_AS_MAP` rejects requests that
  combine `PROT_WRITE` and `PROT_EXEC` and returns `EPERM`.
- The policy applies uniformly to mappings requested by the caller and those created by kernel
  helpers (for example, guarded stacks installed during `spawn`).

## BootstrapMsg (child bootstrap payload)

The kernel sends a single bootstrap message to the child's seeded endpoint on `spawn`.
The payload layout is stable and `#[repr(C)]`:

```rust
#[repr(C)]
pub struct BootstrapMsg {
    pub argc: u32,
    pub argv_ptr: u64,   // child VA (string table); 0 in MVP
    pub env_ptr: u64,    // child VA; 0 in MVP
    pub cap_seed_ep: u32,// initial endpoint handle granted to the child
    pub flags: u32,      // reserved
}
```

Golden layout tests assert size/padding correctness.

## Spawn semantics (dedicated address spaces)

- Default behaviour: a zero `as_handle` argument instructs the kernel to create a fresh Sv39
  address space for the child. The kernel maps a four-page RW stack capped by an unmapped guard
  page and activates the new AS during scheduling.
- Custom handle: callers may bind the child to an existing address space by passing a non-zero
  handle obtained via `SYS_AS_CREATE`. The caller is responsible for provisioning the stack in
  that address space.
- Entry checks: `entry_pc` must lie within `__text_start..__text_end` and be aligned; otherwise
  `SpawnError::InvalidEntryPoint` is raised.
- Spawn failure taxonomy: kernel classifies failures into `SpawnFailReason` (RFC-0013) and exposes
  a bounded reason code via `spawn_last_error` for userland diagnostics.
- Cap table: the child receives a copy of the parent's provided bootstrap endpoint into slot `0`
  (rights are intersected with the mask).
- Bootstrap: the kernel enqueues one IPC to endpoint `0` with a zeroed `BootstrapMsg` payload.
- Trapframe: the child resumes in S/U-mode at `entry_pc` with `sp` pointing at the guarded stack
  top (or the caller-provided stack pointer when using a custom address space).

## Stage policy and selftests (OS path)

- Early boot forbids heavy formatting/allocations; only raw UART writes until selftests run.
- Selftests execute on a private, guarded stack (RW pages bracketed by unmapped guards); timer IRQs
  are masked during the run.
- UART markers (subset): `KSELFTEST: as create ok` → `KSELFTEST: as map ok` →
  `KSELFTEST: child newas running` → `KSELFTEST: spawn newas ok` → `KSELFTEST: w^x enforced` →
  `KSELFTEST: spawn reasons ok` → `KSELFTEST: resource sentinel ok`.
- Note: allocator pressure can still trigger `ALLOC-FAIL` until the cooperative OOM watchdog
  in `TASK-0228` lands; use boot-gate markers for early diagnosis.
- Bring-up diagnostics: illegal-instruction traps print `sepc/scause/stval` and instruction bytes;
  optional `trap_symbols` resolves `sepc` to `name+offset`. A post-SATP marker verifies return.
- Feature gates:
  - Default: `boot_banner`, `selftest_priv_stack`, `selftest_time`.
  - Optional: `selftest_ipc`, `selftest_caps`, `selftest_sched`, `trap_symbols`, `trap_ring`,
    `debug_stack_guards`.

## Structured logging

- Kernel diagnostics flow through lightweight log macros that annotate each line with a severity
  and `target` module: `[INFO mm] map kernel segments ok`.
- `ERROR`, `WARN`, and `INFO` logs are always emitted; `DEBUG`/`TRACE` only compile in debug builds
  (`debug_assertions`) to avoid noise on production runs.
- Required acceptance markers (e.g., `KSELFTEST: …`, `AS: post-satp OK`) remain intact as part of
  the structured messages so CI can continue to grep for them.

## Trap symbolization (opt-in)

When the `trap_symbols` feature is enabled, the build script emits a compact
`TRAP_SYMBOLS: &[(usize, &str)]` table into `.rodata`. Illegal-instruction logs
lookup the nearest symbol to `sepc` and print `name+offset` for debugging. This
has zero runtime overhead when the feature is disabled.

## IPC Header

NEURON exchanges messages using a fixed 16 byte header declared in
`ipc::header::MessageHeader`:

```text
+-------+-------+------+--------+-----+
| src:u32 | dst:u32 | ty:u16 | flags:u16 | len:u32 |
+-------+-------+------+--------+-----+
```

Payload bytes are stored inline in the queue and truncated to `len`
bytes when the message is created.

## Capability Invariants

- Every capability belongs to exactly one task-local table.
- Derivation intersects rights with the parent capability. Rights can
  never be escalated.
- Endpoint capabilities must contain the `SEND` or `RECV` right to
  access queues. VMO capabilities require the `MAP` right to install
  mappings.
- Capability slots are pre-sized per task (96 entries for the bootstrap
  task).

## Scheduler Overview

The scheduler implements a round-robin policy with QoS hints. Tasks are
queued in four buckets (`Idle`, `Normal`, `Interactive`, `PerfBurst`).
When `yield` is invoked the current task is placed at the tail of its
bucket and the highest priority non-empty bucket is dequeued.

SMP v1 status (`TASK-0012`):

- Runtime scheduling remains intentionally single-CPU for deterministic bring-up parity (`SMP=1` unchanged).
- Per-CPU runqueues and bounded work-steal are implemented behind selftest-only APIs and validated by SMP-gated markers.
- Secondary harts run a bounded auxiliary loop while S_SOFT trap handling owns IPI acknowledgement/evidence (no polling-based fake-positive ack path).
- IPI success markers require a strict causal chain (`request accepted -> send_ipi success -> S_SOFT trap observed -> ack`), plus counterfactual reject markers for forced-failure paths.

SMP v1b hardening status (`TASK-0012B`):

- Scheduler QoS queues now enforce explicit bounded capacity with deterministic reject semantics on saturation (no unbounded growth in hot paths).
- S_SOFT resched handling is encapsulated as one explicit contract path (`record trap -> consume pending -> ack when pending`), preserving TASK-0012 marker meaning.
- `cpu_current_id()` uses a guarded hybrid identity path (`tp` hint -> stack-range verification/fallback -> BOOT fallback), keeping CPU/Hart newtypes authoritative and avoiding raw-ID routing.

### Ownership Model (Rust-Specific, pre-SMP)

This section is the ownership contract for TASK-0011B (docs-first) and the pre-SMP baseline
used by TASK-0012.

**Global kernel state shape**:

- `KERNEL_STATE: MaybeUninit<KernelState>` is initialized once in `kmain` and then lives for
  the full kernel lifetime.
- `KernelState` owns the major subsystems directly: `hal`, `scheduler`, `tasks`, `ipc`,
  `address_spaces`, `kernel_as`, `syscalls`.
- Runtime subsystem mutation remains serialized through one scheduler owner (CPU0), while
  secondary harts are online and participate in bounded SMP selftest proof paths.

**Subsystem ownership (current, single-hart)**:

- **`Scheduler`**:
  - Owns scheduler-local runqueues (`VecDeque<Task>`) and current scheduling state.
  - Mutated only through `&mut Scheduler`; no cross-thread/cross-hart mutation today.
- **`TaskTable`**:
  - Owns all task entries (task metadata, trapframes, per-task cap table ownership).
  - PID lifecycle authority is centralized here.
- **`AddressSpaceManager`**:
  - Owns address-space slots, page-table ownership, and ASID allocation state.
  - Address-space mutations are centralized here.
- **`Router` (`ipc`)**:
  - Owns endpoint queues and queue-budget accounting.
  - Send/recv mutate router state through explicit `&mut Router` access in kernel paths.

**Borrowing and lifetime model**:

- Long-lived kernel state is `'static` via `KERNEL_STATE`.
- Syscall/trap handling takes short-lived mutable borrows into owned subsystems.
- No shared mutable state without explicit synchronization primitives.
- Kernel-owned mutable subsystems (`Scheduler`, `TaskTable`, `AddressSpaceManager`, `Router`,
  `CapTable`, `PageTable`) are intentionally `!Send`/`!Sync` and guarded by compile-time tests
  plus explicit ownership boundaries.

**Scheduler ownership split for TASK-0012 (explicit SMP v1 boundary)**:

- **Implemented in v1**:
  - runtime runqueue ownership remains CPU0-local to preserve deterministic `SMP=1` behavior,
  - per-CPU runqueue + bounded steal logic exists as selftest-only proof surface.
- **Implemented in v1b hardening**:
  - runtime enqueue paths are explicitly bounded with deterministic reject/backpressure semantics,
  - CPU identity derivation is explicit and auditable (`tp` advisory fast path + deterministic fallback contract).
- **Remains globally coordinated**:
  - PID namespace/allocation and global task metadata (`TaskTable`),
  - address-space registry and ASID allocator (`AddressSpaceManager`),
  - IPC endpoint namespace and queue-budget authority (`Router`).
- Cross-CPU coordination uses explicit atomic/IPI boundaries (`cpu_online_mask`, resched mailbox/ack),
  never unsynchronized shared mutable runqueue access.

See `docs/architecture/16-rust-concurrency-model.md` for detailed SMP ownership design.

### Implemented vs. Aspirational (avoid doc drift)

This section mixes **current structure** and **planned SMP structure**. To keep the docs honest:

- **Implemented today (SMP v1 baseline)**:
  - single runtime `Scheduler` owner (CPU0) to preserve deterministic behavior,
  - secondary hart bring-up via SBI HSM + per-hart trap stack/`sscratch` handling,
  - SMP proof path via gated markers (`KINIT: cpu1 online`, `KSELFTEST: smp online ok`, `KSELFTEST: ipi counterfactual ok`, `KSELFTEST: ipi resched ok`, `KSELFTEST: work stealing ok`, and `test_reject_*` negative markers).

- **Planned for SMP v2+ (tasks)**:
  - full runtime per-CPU scheduler ownership beyond selftest proof surface,
  - explicit locking/atomic boundaries for shared state (TASK-0277 policy),
  - affinity/shares/QoS controls (TASK-0042, TASK-0013) gated by `policyd`.

When implementation differs from the plan, the task (not this doc) is the authority; update this section
once the implementation lands.

### Construction authority (Rust handles / newtypes)

Rust newtypes are only maximally useful when it is clear **who is allowed to construct them** (authority),
and when the API makes incorrect construction hard:

- **`Pid`**: constructed by `TaskTable` only (kernel authority).
- **`Asid`**: allocated/freed by `AddressSpaceManager` only.
- **`AsHandle`**: created by the `as_create` syscall path; opaque to callers; never reveals the ASID.
- **`CapSlot`**: indexes a per-task cap table; validated at syscall boundaries.
- **`HartId` / `CpuId`**: hardware-vs-logical CPU identity split; SMP paths avoid raw integer IDs in scheduler/boot code.

See `source/kernel/neuron/src/types.rs` for the current newtypes and comments, and keep constructors scoped
so invariants stay enforceable.

## HAL Snapshot

The HAL exposes traits for timers, UART, MMIO, IRQ control and TLB
invalidation; `hal::platform::Machine` bundles the implementations, every one
of them reading its numbers from the statics the device tree filled (see
"Platform from the device tree"). There is no per-machine HAL: QEMU `virt` and
the reference board differ only in their trees.

## Testing Strategy

- Host-based unit tests validate message header layout, scheduler
  ordering and syscall send/recv semantics using the in-memory router.
- `just qemu` (backed by `scripts/run-qemu-rv64.sh`) launches
  `qemu-system-riscv64` with the freshly built kernel archive to confirm
  the boot banner and trap setup execute without crashing.

For deterministic QEMU acceptance (marker contract + ordering), use the canonical harness:

- `scripts/qemu-test.sh` (contract implementation)
- `docs/testing/README.md` (methodology + marker guidance)
