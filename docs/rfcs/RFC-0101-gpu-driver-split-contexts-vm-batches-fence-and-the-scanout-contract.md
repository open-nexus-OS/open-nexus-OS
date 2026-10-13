# RFC-0101: The GPU driver split and the scanout contract — contexts with their own GPU VM, buffers bound with a cache mode, one batch per crossing, the kernel's timeline fence, and a ring of planes flipped at the vertical blank

- Status: In Progress (Phase 0 ✅ 2026-10-12 — the contracts, host-tested; Phase 1 ✅ 2026-10-12 — the scanout contract on the board, `[PASS] board-visible` with `front=0`; Phase 2 ✅ 2026-10-13 — the device online on the board, a null job's round trip 131 µs against the stock 640 µs; Phases 3–4 with their layers)
- Owners: @gpu @runtime @kernel-team
- Created: 2026-10-12
- Last Updated: 2026-10-12
- Links:
  - Tasks: `tasks/TASK-0280-gpu-and-scanout-base-contracts-driverkit-v2-gfxdevice-v2-host.md` (Phase 0, the SSOT ledger of Block 4 layer 1), `tasks/TASK-0216-…` (Phase 1, the scanout contract on the board), `tasks/TASK-0329-…` (Phase 2, the device online), `tasks/TASK-0169B-…` / `TASK-0170B-…` / `TASK-0171-…` (Phase 3, the GPU draws), `tasks/TASK-0215-…` + `TASK-0216-…` (Phase 4, the desktop on the GPU)
  - ADRs: `docs/adr/0018-driverkit-abi-versioning-and-stability.md`, `docs/adr/0032-gpu-command-ring-and-pipelined-present.md`, `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
  - Related RFCs: RFC-0033 (soft-real-time spine: waitset, the timeline fence, driver-kit), RFC-0067 (windowd's compositor boundary), RFC-0098 C4 (page-backed VMOs, `ContiguousDma`, the coherence attribute), RFC-0106 (SoC glue: the GPU's power set)
  - Measurements: `docs/board/measurements/2026-10-10-mgs-baseline/`, `docs/board/measurements/2026-10-10-gpu-stock-baseline/`
  - Order: `tasks/IMPLEMENTATION-ORDER.md` "Block 4 in layers"

## Status at a Glance

- **Phase 0 (the contracts, host)**: ✅ 2026-10-12 — `nexus-driverkit::gpu` (the device side: queue kinds, cache modes, the GPU VM, capabilities, the batch bound), `nexus_gfx::sync::timeline` (the one fence, modelled), `nexus_gfx::backend::v2` (`GfxDevice` with `CpuMockDevice` implementing it, the plane ring, the frame clock and the pointer plane as models), the `test_reject_*` matrix
- **Phase 1 (the scanout contract on the board)**: ✅ 2026-10-12 — TASK-0216: the ring over the layout's three slots, the vertical-blank interrupt as the frame clock (enabled only while a flip waits), the flip inside the blank (first at line 1080), the deferred ack, the pointer plane whole at every border; `[PASS] board-visible` 49 rungs, `front=0` in every present window, `late=0`. Open thread: a second interrupt 21 µs after a served one (`docs/board/measurements/2026-10-12-scanout-vblank/`, H1), measured in Phase 2's first cycle.
- **Phase 2 (the device online)**: ✅ 2026-10-13 — TASK-0329 G1: the GPU node through socd (domain 2 by its software sequence), the published firmware started by the open sequence (`gpu fw ok`), one address space bound and flushed (`gpu mmu ok`), a null job's completion bridged to the kernel fence 64 times (`gpu fence ok (rt_us=131 … below the stock 640us)`); five board cycles, each decided by a line (`docs/board/measurements/2026-10-13-gpu-online/`). Open threads handed on: the uncached mapping for several commands in flight (M2), the firmware's boot-time variance (Phase 3's first cycle)
- **Phase 3 (the GPU draws)**: ⬜ layer 4 — G2 on the device, the first textured quad from offline shaders
- **Phase 4 (the desktop on the GPU)**: ⬜ layer 5 — G3's remaining ops, G4

Definition: "Complete" means the contracts below are implemented on the board, their gates are green, and the CPU compositing path and the v1 backend trait are deleted with a retired-name gate.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in the tasks above.

- **This RFC owns**:
  - the split between gpud (the thin device side), `nexus-driverkit` (what every accelerator server shares) and `nexus-gfx` (what a client of the GPU uses);
  - the client's contract with the GPU: contexts, buffer objects, binding, submission, sync, capabilities, GPU code as data;
  - the scanout contract between the compositor and the display: the ring of planes, the flip at the vertical blank, the frame clock, damage per plane, the pointer plane;
  - the invariants and the negative tests that prove them, and the gates of each phase.
- **This RFC does NOT own**:
  - the device's registers, firmware and bring-up (TASK-0329 and its measurement folders);
  - the compositor's policy — what it draws, caches and puts on a plane (RFC-0067, G4);
  - the memory object model (RFC-0100: demand paging, purgeable, accounting — M2–M5), the fence syscalls themselves (RFC-0033), the display controller's register model (`nexus_gfx::backend::dc`);
  - the graphics/compute API and the array framework above this driver — libraries, later.

### Relationship to tasks (single execution truth)

Each phase names its task; the task holds the stop conditions and the proof commands. Phase 0's gate is `just test-host` green with the reject matrix of this RFC's §9 present by name.

## Context

Measured 2026-10-10 (the two folders above): on the board a present is a CPU composite drawn straight into the plane the controller scans — 150 ms median, every one of them visible half-drawn; the same scene through the stock GPU stack costs 60 ms, or 23 ms with one quarter-size blur; a frame's fixed cost there is 0.64 ms, opaque fill 1.8 ns a pixel, a blended full-screen layer 1.8–2.7 ms. Two conclusions set this contract: the drawing must leave the scanned plane (a ring, flipped at the vertical blank), and the compositor must ask the hardware for the damage, not the frame (damage per plane, blur cached at reduced size, planes for what moves). Today gpud runs one context, only windowd has a route, the v1 backend trait knows no buffers, no binding, no queue kinds and a boolean fence, and the GPU's node is `dma-noncoherent`.

The operator's requirement (2026-10-10): an explicit graphics/compute API and an array/ML framework will sit on this driver later; the driver's base must not make them expensive. That is the seven requirements of §1–§7, from `tasks/TASK-0329`.

## Goals

1. A base every later package is built from — a model that builds G1, G2, G3 or the scanout package reads this contract and its host types, not the plan.
2. Isolation by the GPU MMU, never by parsing command streams.
3. One sync primitive, one buffer vocabulary, one command vocabulary — no second of any.
4. A frame that is never seen half-drawn, and a compositor that pays for its damage.

## Non-Goals

Vendor command encodings (G3's offline pipeline), an on-device shader compiler, implicit sync, a per-dispatch syscall, a software pointer in a present.

## The contract

### §1 Contexts — one per client, with its own GPU address space

A **context** (`ContextId`) belongs to the client (identity = `sender_service_id`, never a payload field) that created it and carries its own GPU address space (`nexus_driverkit::gpu::VmSpace`), its priority and its limits. A client may hold several contexts; windowd's is created at the highest priority. Nothing a context submits can name memory outside its own address space: a reference to an unbound address is refused before anything reaches the device (`test_reject_submit_references_unbound_va`). A context is destroyed with everything bound in it; a submit on a destroyed context is refused (`test_reject_destroyed_context`).

### §2 Queue kinds — render, compute, transfer

`QueueKind::{Render, Compute, Transfer}` is part of every submit. The capabilities say which kinds a device executes; a kind the device does not execute is refused **by name** (`Unsupported`), never silently run on another queue (`test_reject_compute_queue_until_built`). G1 declares compute and refuses it; a later phase executes it without a change of contract.

### §3 Buffers — VMOs bound with a cache mode

A **buffer object** (`BoId`) is a VMO (page-backed, `Anon` or `ContiguousDma`, RFC-0098 C4) created with a declared **cache mode** — `CacheMode::{CpuCached, Uncached, GpuPrivate}` — that the type requires: there is no way to create a buffer without one. `CpuCached` memory is cleaned/flushed at the ownership transitions of `nexus_driverkit::DmaBuffer` (the non-coherent machine's rule; the coherent machine pays nothing); `Uncached` memory needs no maintenance; `GpuPrivate` memory is never mapped for the CPU. A buffer is bound into a context's address space with `vm_bind(ctx, bo, va, access)` and unbound with `vm_unbind`; the space refuses overlapping, unaligned and out-of-range bindings and a binding of another client's buffer (`test_reject_bind_overlapping_range`, `test_reject_unaligned_binding`, `test_reject_bind_outside_space`, `test_reject_bind_bo_of_another_client`). Scatter-gather is the VMO's `vmo_runs`. Bound memory is charged to the client (M4 charges it; this RFC names it).

### §4 Submission — one batch, one crossing

`submit(ctx, kind, Batch) -> TimelinePoint`. A **batch** is the unit of one IPC round trip and (in the device server) one kernel crossing: a stream of work, the fence points it waits for, the point it signals. The stream is either the one command vocabulary this tree has (`nexus_gfx::command::Command`, executed by the CPU, the virtio and the mock devices) or **GPU code as client data** — finished binaries at a bound address with a declared target (§7). A batch is bounded by the device's `max_batch_commands`; a larger one is refused (`test_reject_oversized_batch`). The device server never issues a syscall per dispatch.

### §5 Sync — the kernel's timeline fence

The only synchronization primitive is the kernel's timeline fence (RFC-0033: a monotonic `u64`, `fence_signal` / `fence_wait`, a capability). `TimelinePoint(u64)` is a value on it; a batch waits on points and signals one; the device signals completion; CPU code signals and waits too, and a fence travels to another service as a capability. `nexus_gfx::sync::Timeline` is the host model of it: a signal to a lower value is a no-op (`test_reject_signal_backwards_is_a_no_op`), a wait is satisfied exactly when the value reached the point. The v1 boolean `Fence` is retired with the v1 trait in Phase 3.

### §6 Priority, preemption, timeout and reset — per context

Each context has a `Priority::{Realtime, High, Normal, Low}` (windowd realtime), a timeout per batch and a reset that affects only that context (its in-flight work is cancelled, its points signalled with an error, its space kept). Whether the firmware preempts between contexts is measured in Phase 2 and recorded in the capabilities; the contract does not promise preemption it has not measured.

### §7 GPU code is client data; capabilities are queryable

The driver holds no built-in list of kernels or shaders. `Shader { target, entry, bytes }` is bytes the client supplies (built offline on the host now; a JIT later is a library above this driver), with a declared target the device checks against its own. `Capabilities { queue_kinds, max_contexts, max_batch_commands, va_bits, page_size, cache_modes, gpu_code_target }` is readable before the first context, and a client builds nothing it was not told the device can do.

### §8 The scanout contract — a ring of planes, flipped at the vertical blank

Between the compositor and the display:

- The display scans a **plane ring** (`PlaneRing<N>`, N ≥ 2 — three on the board). A slot is `Free`, `Rendering` (a client draws into it), `Queued` (a flip is requested) or `Scanned` (the controller reads it). **The scanned slot is never a render target** (`test_reject_render_into_scanned_slot`).
- A **flip** names a slot, the damage drawn into it and the point that says the drawing is done; it takes effect at the next **vertical blank**, never before (`a_flip_takes_effect_at_the_vertical_blank_not_before`), and the previous scanned slot becomes free then. The controller's vertical-blank interrupt is the **frame clock**: the compositor's completions, its animations and its pulses to clients are clocked by it, not by the end of a CPU composite.
- **Damage per plane** (buffer age): the ring tells a client how many frames ago a slot was last scanned, so a client redraws into it exactly what changed since — the frame costs its damage, never the screen.
- The **pointer plane** is the controller's own layer: it moves without a flip, its rectangle is clamped whole at every border of the display (`test_reject_pointer_rect_outside_the_display` — the strip at the right border of 2026-10-10), and no present carries a software pointer.
- The wire form of flip and completion (slot, damage rects, the vertical-blank time) extends the display protocol in Phase 1; this RFC fixes the semantics, the types in `nexus_gfx::backend::v2::scanout` are the SSOT of them.

### §8b Phase 2 — the device online (TASK-0329 G1, the thin side's own contract)

What the device server owns below §1–§7 on the board's core (one core of the B-series XE family, id `36.29.52.182`, a MIPS firmware processor, 40-bit GPU VA, 36-bit bus, non-coherent), with the open kernel driver's documented interface as the reading list and the stock system's live words as the expected values:

- **The firmware is the device's, not a shader.** It is loaded unmodified from `resources/firmware/gpu/` by its documented image layout (an ELF32 in 4 KiB blocks with a trailer: layout table, device-info masks, id, version, the open-build flag); a blob whose id, flag or version differs is refused before a byte reaches memory (`test_reject_firmware_wrong_id`, `test_reject_firmware_not_open_build`, `test_reject_firmware_bad_trailer`). The device's `Capabilities` are filled from the blob's device-info block, never from a table of ours.
- **Firmware memory** is a 16 MiB heap at GPU VA `0xE1_C000_0000` (the FIRMWARE heap; the other fixed heaps: GENERAL `0x80_0000_0000`, PDS `0xDA_0000_0000`, USC `0xE0_0000_0000`, TRANSFER_FRAG `0xE4_0000_0000`), mapped by the firmware's own page table that the host builds from page-backed VMOs (`vmo_runs` with the GPU's device capability — a run outside the device's reach is the kernel's refusal); the top 192 KiB are three fixed 64 KiB slots (connection control, OS init, SYS init). Every word the firmware reads or writes is device-uncached and, on the CPU, uncached or Zicbom-maintained at the `DmaBuffer` transitions (§3).
- **The start sequence** is the open driver's for this feature set (secure bus off, the two reset words, rascal/dusts out, everything out but the firmware processor, SLC decode mode, the MIPS wrapper and its remaps — including the extra remap for address 0 the core's quirk needs —, the bus-interconnect word, reset released), recorded over `RegWriter` on the host and compared with the stock words where both write (`SLC_CTRL_MISC 0x200003`, `MIPS_WRAPPER_CONFIG 0x1CF80`, the remap pairs). Liveness is `firmware_started` observed after the first interrupt — a bounded wait on the waitset, never a spin.
- **A context's address space is one page catalogue** (three levels: catalogue 1024 × 32-bit, directory 512 × 64-bit, table 512 × 64-bit for 4 KiB pages); the firmware memory context names it and the firmware assigns the hardware slot when it schedules the context — the host never writes a catalogue-base register for the MIPS firmware. `VmSpace` (§3) is the bookkeeping; the table encoder is pure and host-tested (`test_reject_unaligned_va`, `test_reject_va_beyond_40_bits`). Every table change is followed by a firmware MMU-cache command whose return slot is waited for; an unmap is always followed by a waited flush; a flush that times out is a firmware lock-up (named, stopped; the reset path is Phase 3's).
- **Submission rings**: one kernel command buffer (128 commands + a return slot each; the firmware sets EXECUTED), one firmware-to-host buffer (32), and per queue a client command buffer (32 KiB; a command never wraps) with a firmware common context (`dm`, `priority`, `max_deadline_ms`, the memory context, a register-state save area) and one 32-bit timeline word. A kick is the commands written in the device server's memory and ONE register write (`MTS_SCHEDULE`) — §4's one crossing.
- **The fence bridge** (§5): the firmware's timeline word per queue is read by the interrupt handler and `fence_signal`led onto the kernel timeline fence of that queue; completion is "word ≥ point", never a per-job record. The interrupt handler clears, drains, re-checks (an event during processing is never lost); the firmware's own interrupt count is compared with ours in the gate line.
- **Power** through socd (RFC-0106): the GPU node's plan is domain 2 by its software sequence (sleep1, sleep2, isolation released, the status bit polled with a bound), the core clock at the node's rate, the reset released at least 32 clock cycles after the clock. gpud may bring up only GPU and display nodes (RFC-0106's per-class floor, first consumer).
- **Gates**: `gpud: gpu glue ok (`, `gpud: gpu core ok (bvnc=…)`, `gpud: gpu fw ok (boot_us= irqs= fw_irqs=)`, `gpud: gpu mmu ok (`, `gpud: gpu fence ok (rt_us= n=)` REQUIRED on the board ladders; the null job's round trip below the stock stack's 0.64 ms frame fixed cost (`docs/board/measurements/2026-10-13-gpu-online/`). QEMU unchanged. Preemption between contexts is measured when two contexts run (Phase 3's first cycle), as §6 says.

## Invariants (security)

- Identity is `sender_service_id`; a context, a buffer and a fence are reachable only by the client that owns them or was handed the capability.
- Every size is bounded before use: batches by `max_batch_commands`, bindings by the space and the page size, damage rects by the plane, the pointer rect by the display.
- No command stream is parsed for safety — the GPU MMU isolates; the device server validates references against the context's space, nothing else.
- MMIO stays `USER|RW`, never exec; the device server receives its windows from init's tree walk.

## §9 The reject matrix (Phase 0's gate, by name)

`nexus-driverkit::gpu`: `test_reject_bind_overlapping_range`, `test_reject_unaligned_binding`, `test_reject_bind_outside_space`, `test_reject_too_many_bindings`, `test_reject_lookup_of_an_unbound_va`, `test_reject_batch_over_the_capability`. `nexus_gfx::sync::timeline`: `test_reject_signal_backwards_is_a_no_op`. `nexus_gfx::backend::v2::cpu_mock`: `test_reject_bind_bo_of_another_client`, `test_reject_submit_references_unbound_va`, `test_reject_compute_queue_until_built`, `test_reject_oversized_batch`, `test_reject_destroyed_context`, `test_reject_gpu_code_for_another_target`. `nexus_gfx::backend::v2::scanout`: `test_reject_render_into_scanned_slot`, `test_reject_flip_of_a_free_slot`, `test_reject_pointer_rect_outside_the_display`, `test_reject_damage_outside_the_plane`.

## Phases and gates

| Phase | Layer | Delivers | Gate |
|---|---|---|---|
| 0 | 1 | this contract and its host types, the mock device implementing them | `just test-host` green; the matrix of §9 by name; no OS code |
| 1 | 2 | the plane ring on the board's controller, the vertical-blank interrupt, the frame clock, damage per slot, the pointer plane whole at every border; the wire form | `front=0` in every `gpud: dc present us` window; flips = presents; operator: no flicker, shadows stable |
| 2 | 3 | the device online: firmware, GPU MMU with `VmSpace`, power through socd, interrupt, the fence bridge — ✅ 2026-10-13 | `gpud: gpu fw ok`, `gpud: gpu mmu ok`, `gpud: gpu fence ok (rt_us=131 …)`: the null job's round trip below the stock 0.64 ms (board, cycle 5) |
| 3 | 4 | `GfxDevice` on the device: buffers, binding, batches; the first textured, blended quad from offline shaders into a slot of the ring; the v1 trait and the boolean fence retired | a textured quad on the board; opaque fill at the stock 1.8 ns a pixel |
| 4 | 5 | the desktop: damage-driven compositing, cached reduced-size blur, planes for what moves; the CPU composite deleted on the board | our frame of the gate scene below the stock 23.3 ms; a hover frame below 8.3 ms; `board-visual: gpu-desktop` |

## Open questions (decided by measurement, not here)

- The firmware's preemption and priority levels (Phase 2's first cycle) — until measured, §6 promises a reset, not a preemption.
- The MMU page size the device is configured with (4K to 2M supported by the family; one size chosen in Phase 2).
- Whether the transfer queue exposes the destination to a shader (needed for blur without a render-target dataset) — Phase 3's first cycle.
