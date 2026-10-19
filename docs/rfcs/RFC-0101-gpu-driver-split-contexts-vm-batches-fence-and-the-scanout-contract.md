# RFC-0101: The GPU driver split and the scanout contract — contexts with their own GPU VM, buffers bound with a cache mode, one batch per crossing, the kernel's timeline fence, and a ring of planes flipped at the vertical blank

- Status: In Progress (Phase 0 ✅ 2026-10-12 — the contracts, host-tested; Phase 1 ✅ 2026-10-12 — the scanout contract on the board, `[PASS] board-visible` with `front=0`; Phase 2 ✅ 2026-10-13 — the device online on the board, a null job's round trip 131 µs against the stock 640 µs; Phases 3–4 with their layers)
- Owners: @gpu @runtime @kernel-team
- Created: 2026-10-12
- Last Updated: 2026-10-16
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
- **Phase 3 (the GPU draws)**: ✅ 2026-10-13 — TASK-0169B: `GfxDevice` on the device (one context, buffers as DMA VMOs or an adopted plane, binding through the G1 page tables with the waited flush, one transfer command + `UPDATE` per submit); the open driver's two transfer jobs for this core captured on the host as the scene bundle (§8c) and replayed into a plane of the ring on the first board cycle: `gpu fill ok (ns_per_px=0.8 … the stock 1.8)`, `gpu quad ok (samples=4/4)`, `[PASS] board-visible` 57 rungs (`docs/board/measurements/2026-10-13-gpu-first-quad/`). Handed on: blending and the render queue (Phase 4), the firmware's time variance and the controller's alpha handling (Phase 4's first cycle)
- **Phase 4 (the desktop on the GPU)**: ⏳ layer 5, in progress since 2026-10-13 (TASK-0215 recut) in four steps (§8d) — **4a ✅ 2026-10-14**: the render queue on the board — the captured render pass (geometry, partial-render fragment, fragment) replayed with the render context, the free lists and the dataset built by the device; four cycles, three reds each named by a line, then `gpud: gpu render ok (us=6133 samples=4/4 irqs=3 geom=1 frag=2)`, `[PASS] board-visible` 58 rungs (`docs/board/measurements/2026-10-13-gpu-render-queue/`); **4b ✅ 2026-10-14**: dynamic geometry — three quads at positions and tints the device chose, every per-draw word written by the device from the open driver's word layouts, every pass in fresh memory, the two render-target data alternating; six cycles, four reds each named, then `gpud: gpu quads ok (n=3 us=13096 samples=3/3)` — about 3.5 ms per 512² blended quad, 4c's first measurement; 4c in progress — **4c-1 ✅ 2026-10-14**: the cost of a pass — every buffer mapped by the open driver's creation flags, a pass limited to its damage (`gpud: gpu area ok (us=410 full_us=2538 … near=kept far=kept)`), and a pixel priced by its fragment program: 0.15 ns + 0.206 ns per instruction group, the open compiler's 55–68 groups for a textured quad against about eight for the stock's fill; three cycles, `[PASS] board-visible` 60 rungs; **4c-2 ✅ 2026-10-16**: the compositor's programs ours — WGSL through our compiler into a validated library, on the board and on QEMU (ADR-0072); 4c-3 the frame — the mixed pass, several contexts with a fault kept in its own, windowd's context over IPC (§7b), **the desktop on the board's GPU ✅ 2026-10-19**: windowd's frames drawn in its context from the frame plan both paths share, 13–18 ms a frame with its glass cached and 24–88 ms while glass is written against the CPU path's 101–231 ms; next the damage-driven frame; 4d the CPU composite deleted

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

A **context** (`ContextId`) belongs to the client (identity = `sender_service_id`, never a payload field) that created it and carries its own GPU address space (`nexus_driverkit::gpu::VmSpace`), its priority and its limits. A client may hold several contexts; windowd's is created at the highest priority. Nothing a context submits can name memory outside its own address space: a reference to an unbound address is refused before anything reaches the device (`test_reject_submit_references_unbound_va`). A context is destroyed with its bindings and the device's objects for it — the client's buffers stay the client's; a submit on a destroyed context is refused (`test_reject_destroyed_context`). On the board's device (TASK-0215 5c-3 P2) each context is its own page catalogue and firmware memory context, numbered by slot and generation (a destroyed number stays refused when its slot serves again), its objects let go by the firmware's cleanup in the open kernel driver's order before any of its memory is reused — and kept, never freed, when the firmware will not let go.

### §2 Queue kinds — render, compute, transfer

`QueueKind::{Render, Compute, Transfer}` is part of every submit. The capabilities say which kinds a device executes; a kind the device does not execute is refused **by name** (`Unsupported`), never silently run on another queue (`test_reject_compute_queue_until_built`). G1 declares compute and refuses it; a later phase executes it without a change of contract.

### §3 Buffers — VMOs bound with a cache mode

A **buffer object** (`BoId`) is a VMO (page-backed, `Anon` or `ContiguousDma`, RFC-0098 C4) created with a declared **cache mode** — `CacheMode::{CpuCached, Uncached, GpuPrivate}` — that the type requires: there is no way to create a buffer without one. `CpuCached` memory is cleaned/flushed at the ownership transitions of `nexus_driverkit::DmaBuffer` (the non-coherent machine's rule; the coherent machine pays nothing); `Uncached` memory needs no maintenance; `GpuPrivate` memory is never mapped for the CPU (on the board: a VMO named by `vmo_runs` alone, no region of the device server's address space). A **range** of a buffer is bound into a context's address space with `vm_bind(ctx, Mapping { bo, offset, len, va, access })` — a client binds ranges of a few large buffers, and a buffer may be bound in several ranges and contexts of its client — and unbound with `vm_unbind`; the space refuses overlapping, unaligned and out-of-range bindings, a range past its buffer and a binding of another client's buffer (`test_reject_bind_overlapping_range`, `test_reject_unaligned_binding`, `test_reject_bind_outside_space`, `test_reject_mapping_past_the_buffer`, `test_reject_bind_bo_of_another_client`). A buffer is destroyed by its client with `destroy_bo` once nothing binds it (`test_reject_destroy_bound_bo`). Scatter-gather is the VMO's `vmo_runs`. Bound memory is charged to the client (M4 charges it; this RFC names it).

### §4 Submission — one batch, one crossing

`submit(ctx, kind, Batch) -> TimelinePoint`. A **batch** is the unit of one IPC round trip and (in the device server) one kernel crossing: a stream of work, the fence points it waits for, the point it signals. The stream is either the one command vocabulary this tree has (`nexus_gfx::command::Command`, executed by the CPU, the virtio and the mock devices) or **GPU code as client data** — finished binaries at a bound address with a declared target (§7). A batch is bounded by the device's `max_batch_commands`; a larger one is refused (`test_reject_oversized_batch`). The device server never issues a syscall per dispatch.

### §5 Sync — the kernel's timeline fence

The only synchronization primitive is the kernel's timeline fence (RFC-0033: a monotonic `u64`, `fence_signal` / `fence_wait`, a capability). `TimelinePoint(u64)` is a value on it; a batch waits on points and signals one; the device signals completion; CPU code signals and waits too, and a fence travels to another service as a capability. `nexus_gfx::sync::Timeline` is the host model of it: a signal to a lower value is a no-op (`test_reject_signal_backwards_is_a_no_op`), a wait is satisfied exactly when the value reached the point. The v1 boolean `Fence` is retired with the v1 trait in Phase 3.

### §6 Priority, preemption, timeout and reset — per context

Each context has a `Priority::{Realtime, High, Normal, Low}` (windowd realtime), a timeout per batch and a reset that affects only that context (its in-flight work is cancelled, its points signalled with an error, its space kept). Whether the firmware preempts between contexts is measured in Phase 2 and recorded in the capabilities; the contract does not promise preemption it has not measured.

**A fault loses its context alone** (TASK-0215 5c-3 P2c). The device holds the firmware's reset against the context it blames — after a page fault the context whose page catalogue the firmware names, otherwise the context it numbers unless it calls that context innocent — and that context is lost: every new use of it is refused by name (`DeviceError::ContextLost`: submit, bind, reset, render setup), its points count as reached (nothing waits for work that will not run; the loss is named at the context's next use), its memory stays its client's to unbind until the client destroys it and makes a new one. Every other context runs on: the device answers the firmware's recovery requests (the free lists the reset cut off rebuilt as made), and a reset that calls a context innocent holds nothing against it.

### §7 GPU code is client data; capabilities are queryable

The driver holds no built-in list of kernels or shaders. `Shader { target, entry, bytes }` is bytes the client supplies (built offline on the host now; a JIT later is a library above this driver), with a declared target the device checks against its own. `Capabilities { queue_kinds, max_contexts, max_batch_commands, va_bits, page_size, cache_modes, gpu_code_target }` is readable before the first context, and a client builds nothing it was not told the device can do.

### §7b The pipeline library — how GPU code reaches the device (4c-2, ADR-0072)

- **A pipeline** is what the device binds for a draw: its program (shader-core code), the setup
  programs that kick it, the state words it fixes (the pass type, the output format) and its
  interface (inputs, outputs, resources, constants). Programs are compiled offline from WGSL
  through our IR by the code target's backend (ADR-0072); the device never compiles.
- **The library** is one archive per code target: a header (magic, format version, code target,
  entry count, total size) and entries (id, interface, program bytes, setup programs, state
  words, feature bits, budget, digest). Every size and count is bounded and checked before a byte
  is copied (`test_reject_library_truncated`, `test_reject_library_oversize_entry`,
  `test_reject_library_unknown_version`); a library for another code target is refused whole
  (`test_reject_library_wrong_target`); an entry whose digest does not match its bytes is refused
  (`test_reject_library_digest_mismatch`), as is one whose feature bits the device lacks
  (`test_reject_library_unsupported_feature`).
- **The client supplies it** (a VMO) once per context. The device validates it, places the
  programs in memory bound in the client's context (code is a buffer like any other, §3) and
  answers with the entries it took and refused, the first refusal named
  (`GfxDevice::take_library`; `test_reject_second_library_for_a_context`,
  `test_reject_library_into_another_clients_context`); a draw names an accepted id
  (`DeviceError::UnknownPipeline`). The device holds no program of its own and patches no
  program byte; only the setup programs receive the per-draw addresses, as the open driver's do.
- **The wire** (TASK-0215 5c-3 P3, `nexus_display_proto::gpu_context`, windowd → gpud):
  `OP_GPU_CAPABILITIES` (16) answers the code target, its feature bits and the context bound —
  `STATUS_UNSUPPORTED` from a display whose GPU runs no code; `OP_GPU_CONTEXT_OPEN` (17) carries
  the priority and the library's length with the library moved in as a VMO, and answers the
  context and the entries taken and refused. Both answers are sixteen bytes with the op in their
  second byte. Only windowd opens (identity = `sender_service_id`; anyone else `STATUS_DENIED`,
  `test_reject_open_from_another_service`); the frame is decoded fail-closed (a known priority, no
  flag bits, a library of 1 B to 1 MiB, `test_reject_open_frames_of_unknown_flags_priorities_
  lengths_and_bounds`); the VMO is checked before it is mapped (a VMO whose window holds the
  declared bytes, `test_reject_library_vmo_shorter_than_declared_or_of_another_kind`), mapped
  read-only for the take and closed whatever the answer. An open by a client that holds a
  context replaces it whole — a restarted windowd keeps its kernel id and gets a fresh context.
  windowd carries one library per code target and picks it by the target the capabilities name;
  gpud embeds none (a gate fails on one in its code, the board rungs' fixture excepted). On the
  board windowd's context is made as the render rungs' is (the render bundle bound, its target
  left unbound — windowd names every pass's target); on QEMU the one virgl context takes the
  library once a boot, before windowd's attach starts its GL scanout.
- **A budget is part of an entry**: its instruction-group count. The host build fails above it;
  a board rung measures the pipeline's ns a pixel against 4c's line (0.15 ns + 0.206 ns a group).
- **The format keeps the horizon open**: its version and the feature bits leave room for
  bindless resources, indirect work, subgroup and matrix operations, mesh and ray workloads.

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

### §8c Phase 3 — the GPU draws (TASK-0169B, the transfer job and the scene bundle)

- **The first op is a transfer job.** The core's transfer queue runs the fragment pipeline over a
  rectangle: a pixel program from the USC heap, driven by PDS programs from the PDS heap, writing
  through the pixel back end into a linear target — a clear (the colour in `usc_clear_register0`)
  and a filtered, scaled textured copy are both one such job. Its firmware form is the open kernel
  driver's transfer command: 192 bytes — a common header, 17 32-bit registers, 13 64-bit registers
  (`pds_bgnd0/1_base`, `pds_bgnd3_sizeinfo`, `isp_mtile_base`, nine pixel-back-end words), flags
  — filled from a **stream** in the open kernel driver's definition order (8-byte header: length,
  zero padding; the 64-bit words first, the 32-bit words after; `isp_xtp_pipe_enable` only with
  `S7_TOP_INFRASTRUCTURE`, `frag_screen` only with `GPU_MULTICORE_SUPPORT` — the device's own
  device-info block decides, never a constant). `nexus_gfx::backend::gpu::fwif::TransferCmd` is
  that struct with its offsets asserted, `from_stream(bytes, &DeviceInfoBlock)` the decoder
  (`test_reject_stream_length_for_features`, `test_reject_stream_padding_nonzero`), `to_bytes()`
  the encoder. The command goes into the transfer context's CCB as type `TQ_3D` followed by the
  `UPDATE` that writes the timeline — exactly the null job's shape with a payload.
- **GPU code is client data (§7), captured on the host.** The binaries and the streams come from
  the open userspace driver for this core, driven on the host over a shim that stands in for the
  kernel; a hook writes every bound buffer with its GPU address and every job's stream
  (`docs/board/measurements/2026-10-13-gpu-first-quad/`). A **scene bundle**
  (`nexus_gfx::backend::gpu::scene`, file `resources/gpu/scenes/*.scene`, magic `NXSC`) carries the
  core id it was captured for, the buffers as sparse non-zero runs with a kind — `Data` (the
  driver's bytes), `Target` (bound to a plane of the ring, never carried), `Texture` (generated on
  the device by a named formula whose digest is a golden) — and the jobs with their streams. The
  parser treats the file as untrusted (`test_reject_scene_for_another_core`,
  `test_reject_scene_run_outside_buffer`, `test_reject_scene_buffer_outside_heaps`); the device
  refuses a scene whose core id is not its own.
- **The device side is `GfxDevice` (§1–§5) on the G1 device:** `create_bo` is a DMA VMO with its
  cache mode, `vm_bind` the G1 page tables plus the waited MMU-cache flush, `submit` one transfer
  command + `UPDATE`, one kick, one interrupt, one timeline point. The plane ring's VMO is bound by
  its `vmo_runs` at the scene's target address — no copy. Ownership of the plane crosses from the
  CPU to the device before the job (clean) and back before the readback (invalidate), the
  non-coherent machine's rule of §3.
- **Gates**: `gpud: gpu fill ok (us= px=1920x1080 ns_per_px=)` with `ns_per_px` at or below the
  stock 1.8; `gpud: gpu quad ok (us= samples=4/4)` — four sample pixels read back after the blit
  (background, the texture's white frame, both checker colours) — REQUIRED on the board ladders;
  the FAIL forms name the sample (expected, read) or the firmware fault (type, address). QEMU
  unchanged. **Not in this phase:** the render queue, blending (fragment-program work of Phase 4),
  windowd's use of the device over IPC, the v1 trait's retirement (Phase 4, once windowd stands on
  v2), the uncached mapping for several commands in flight (M2).

### §8d Phase 4 — the render queue and the desktop (TASK-0215, the kernel side's objects ours)

- **A render pass is two queues and three commands.** The geometry job (type `GEOM`, 201) builds
  the tile lists from a control stream in the VDM's vocabulary; the fragment job (type `FRAG`,
  203) shades the tiles into the attachment; a partial-render fragment (`FRAG_PR`, 204) stands
  ready should the geometry outgrow its parameter memory. Both run in **one render context**:
  one firmware object holding the geometry's and the fragment's common contexts (§8b's shape,
  data masters GEOM and FRAG) and the **static render-context state** — the 23 context-switch
  registers the open userspace driver emits once per context (a 192-byte stream, decoded as the
  open kernel driver decodes it). The geometry queue's point gates the fragment through the
  firmware's own fence command (`FENCE_PR` on the geometry's timeline word) — the kernel's
  timeline fence (§5) stays the one primitive the host sees.
- **Parameter memory is the device's.** The **free lists** (a local and a global one) are lists
  of the device's own DMA pages — page numbers written by the device, never by a client — with
  the firmware struct the open kernel driver initialises (`ready_pages`, `current_stack_top`,
  the 16-byte-aligned current address); a growth request from the firmware is answered by the
  device or named, never ignored. The **render-target dataset** (two render-target data for the
  two in-flight frames, one common block) is the firmware's view of the attachment's geometry:
  tiles of 16×16, 16 macrotiles for parameter format 2, `te_screen`, `te_mtile1/2`,
  `isp_mtile_size`, `screen_pixel_max`, the single-sample positions — the open kernel driver's
  formulas, host-tested; its memory (tail pointers, macrotile lists, region headers in the
  `RGNHDR` heap) is the client's, bound in the context.
- **The attachment is a plane of the ring, linear.** The open driver accepts a linear colour
  attachment for this core: the pixel back end writes the plane directly, no copy; the texture
  is linear and host-written. A `Stream::Render { geom, frag_pr, frag }` batch (§4) on
  `QueueKind::Render` is refused unless the dataset's and the free lists' addresses resolve in
  the context (`test_reject_render_dataset_unbound`); the streams are accepted only in the
  feature-gated lengths (`test_reject_geom_stream_length`, `test_reject_frag_stream_length`),
  the static state only as the 23 registers (`test_reject_static_state_length`).
- **What moves with the geometry** (the 1-vs-3-quad capture): the vertex buffer, the geometry
  control stream's draw records, the PDS data (per-draw constants) and the descriptor words
  (texture state) — four buffers; everything else, the USC code included, is static. 4b writes
  those four on the device from the open driver's control-stream word layouts carried as data;
  4c builds the compositor's ops on them; 4d deletes the CPU composite.
- **Two rules of a pass, learned on the board (4b, 2026-10-14).** (1) The dataset's two
  render-target data **alternate pass by pass** — the open driver names data 0, then data 1
  (the two-frame capture); a second pass on the same data locked the fragment stage. (2) **A pass
  lands in memory the GPU has not read** — its control stream, vertices, vertex PDS data and every
  draw's blocks in fresh lines, the geometry job's `vdm_ctrl_stream_base` naming the pass's
  stream; a pass that rewrote what the previous pass had read lost its draws (the GPU's caches
  keep lines across kicks). The device's placer bumps through the buffers' unused tails; reuse
  waits for the pass that read a line to be reached on the timeline (4c).
- **What a pass costs, and the choices that follow (4c's first package, measured on the board
  2026-10-14).** (1) **Each buffer is mapped by its creation flags as the open driver sets
  them**: only the dataset, the free lists and the context state stay out of the GPU's
  system-level cache, the parameter manager's and firmware's pages are protected, everything
  else is cached; the render bundle carries the flags per buffer; a plane of the ring stays out
  (the display reads it; caching it gained nothing). Every page out of the cache cost 2–4 % a
  pass and nothing per pixel. (2) **A pass is limited to its damage**: the pixel back end's clip
  (pixel-precise), the region clip (16-pixel tiles), no background on empty tiles — the
  background on every tile of the plane was 2.1 ms of a 2.5 ms pass; outside its rectangle the
  pass writes nothing (gate `gpu area ok`). (3) Pixel merging stays off, as the open driver keeps
  it (no per-pixel gain measured). (4) **What is left per pixel is the fragment program**: 0.21 ns
  an instruction group, almost nothing outside it — the open compiler's 55–68 groups for a
  textured quad cost 11–14 ns against the stock's 1.8 ns opaque fill, about eight groups on the
  same line. 4c's programs are therefore written for this core with a group budget each (§7:
  GPU code stays client data). (5) Every buffer the device creates is a region of gpud's address
  space (64): a rung draws in the bundle's buffers' tails, never in buffers of its own.
- **Gates**: 4a `gpud: gpu render ok (us= samples=4/4 irqs=)`; 4b `gpu quads ok (n= us=)`; 4c
  `gpu area ok (us= full_us= inside=ok near=kept far=kept)`, then `gpu desktop ok (frame_us=)`
  below the stock 23.3 ms and `board-visual: gpu-desktop`; 4d a hover frame below 8.3 ms. QEMU
  unchanged.

## Invariants (security)

- Identity is `sender_service_id`; a context, a buffer and a fence are reachable only by the client that owns them or was handed the capability.
- Every size is bounded before use: batches by `max_batch_commands`, bindings by the space and the page size, damage rects by the plane, the pointer rect by the display.
- No command stream is parsed for safety — the GPU MMU isolates; the device server validates references against the context's space, nothing else.
- MMIO stays `USER|RW`, never exec; the device server receives its windows from init's tree walk.

## §9 The reject matrix (Phase 0's gate, by name)

`nexus-driverkit::gpu`: `test_reject_bind_overlapping_range`, `test_reject_unaligned_binding`, `test_reject_bind_outside_space`, `test_reject_too_many_bindings`, `test_reject_lookup_of_an_unbound_va`, `test_reject_batch_over_the_capability`. `nexus_gfx::sync::timeline`: `test_reject_signal_backwards_is_a_no_op`. `nexus_gfx::backend::v2::cpu_mock`: `test_reject_bind_bo_of_another_client`, `test_reject_mapping_past_the_buffer`, `test_reject_destroy_bound_bo`, `test_reject_submit_references_unbound_va`, `test_reject_compute_queue_until_built`, `test_reject_oversized_batch`, `test_reject_destroyed_context`, `test_reject_gpu_code_for_another_target`. `nexus_gfx::backend::v2::scanout`: `test_reject_render_into_scanned_slot`, `test_reject_flip_of_a_free_slot`, `test_reject_pointer_rect_outside_the_display`, `test_reject_damage_outside_the_plane`.

## Phases and gates

| Phase | Layer | Delivers | Gate |
|---|---|---|---|
| 0 | 1 | this contract and its host types, the mock device implementing them | `just test-host` green; the matrix of §9 by name; no OS code |
| 1 | 2 | the plane ring on the board's controller, the vertical-blank interrupt, the frame clock, damage per slot, the pointer plane whole at every border; the wire form | `front=0` in every `gpud: dc present us` window; flips = presents; operator: no flicker, shadows stable |
| 2 | 3 | the device online: firmware, GPU MMU with `VmSpace`, power through socd, interrupt, the fence bridge — ✅ 2026-10-13 | `gpud: gpu fw ok`, `gpud: gpu mmu ok`, `gpud: gpu fence ok (rt_us=131 …)`: the null job's round trip below the stock 0.64 ms (board, cycle 5) |
| 3 | 4 | `GfxDevice` on the device: buffers, binding, batches; the first textured quad from offline shaders into a slot of the ring (§8c) | `gpud: gpu fill ok (ns_per_px=)` at or below the stock 1.8 ns a pixel; `gpu quad ok (samples=4/4)`; the quad on the board |
| 4a | 5 | the render queue on the board: the captured render pass (clear + one blended textured quad; geometry, partial-render fragment, fragment) replayed through `GfxDevice`, the render context, free lists and render-target dataset built by the device as the open kernel driver builds them (§8d) | `gpud: gpu render ok (us= samples=4/4 irqs=)` — the blended samples on the board |
| 4b | 5 | dynamic geometry: the vertex buffer, the geometry control stream's draw records, the PDS data and the descriptor words written by the device from the open driver's word layouts (data) — N quads at runtime positions, tints, textures | `gpud: gpu quads ok (n= us=)` |
| 4c | 5 | the compositor's ops as pipelines (textured layer with opacity and rounded corners, drop shadow, gradient fill, the blur as two passes at quarter size into a cached surface), the command stream → draw list, damage-driven, the plane as render target | our frame of the gate scene below the stock 23.3 ms; `board-visual: gpu-desktop` |
| 4d | 5 | the CPU composite deleted on the board; a hover frame over cached blur; what moves alone on a controller plane; the v1 trait and the boolean fence retired once windowd stands on v2 | a hover frame below 8.3 ms; the tolerated present-time lines gone |

## Open questions (decided by measurement, not here)

- The firmware's preemption and priority levels (Phase 2's first cycle) — until measured, §6 promises a reset, not a preemption.
- The MMU page size the device is configured with (4K to 2M supported by the family; one size chosen in Phase 2).
- Whether the transfer queue exposes the destination to a shader (needed for blur without a render-target dataset) — Phase 3's first cycle.
