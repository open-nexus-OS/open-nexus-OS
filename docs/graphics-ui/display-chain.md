---
schema: 1
title: "Display chain"
role: guide
subsystem: ui
sources: [source/drivers/gpud/src/backend, source/drivers/gpud/src/service.rs, source/drivers/gpud/src/present_chain.rs, source/services/windowd/src/compositor, source/libs/nexus-display-proto/src]
contracts: [RFC-0093, RFC-0098, RFC-0101, RFC-0110, RFC-0114, ADR-0038, ADR-0078, ADR-0092]
verified: 2026-11-10
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display chain

> **Purpose** — how a frame windowd composed reaches the glass: the grant, the present, the flip and the ack, on QEMU and on the board.
> **Owns** — `source/drivers/gpud/src/backend/`, `source/drivers/gpud/src/service.rs`, `source/drivers/gpud/src/present_chain.rs`, `source/services/windowd/src/compositor/`, `source/libs/nexus-display-proto/src/`
> **Contracts** — [RFC-0093](../rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md) §5 (the gpud wire, the reveal), [RFC-0101](../rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md) §8 (scanout contract), [RFC-0110](../rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md) (frame timing), [RFC-0114](../rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md) / [ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md) (app surfaces), [ADR-0078](../adr/0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md) (GPU memory), [ADR-0038](../adr/0038-display-wire-ssot-and-capnp-boundary.md) (one wire crate)
> **Invariants** — one owner per thing: gpud owns the mode, the scanout memory and the pointer plane; windowd owns the scene and every present. A present is acked only for a seq that is outstanding. The board's display composes nothing on the CPU: a frame the GPU did not draw is refused by name.
> **Read next** — [README.md](README.md), [gpud-ring.md](gpud-ring.md), [nexus-gfx.md](nexus-gfx.md)

## Owners

| party | owns | code |
|---|---|---|
| gpud | display mode (`display_mode::resolve`), framebuffer and ring memory, GPU contexts, app surface buffers, the pointer plane, readback | `source/drivers/gpud/src/` |
| windowd | the layer set, damage, every present and its seq, the frame clock it derives from acks | `source/services/windowd/src/compositor/` |
| wire | every opcode and frame of the windowd↔gpud and app↔windowd channels | `source/libs/nexus-display-proto/src/` |
| selftest-client | observes markers only | `source/apps/selftest-client/` |

gpud's request loop (`service.rs`) drives one display behind the `Display` trait (`backend/display.rs`): QEMU's virtio GPU (`backend/virtio_display.rs`) or the board's controller (`backend/dc/`).

## Bring-up handshake

| step | op (`nexus_display_proto`) | effect | marker |
|---|---|---|---|
| 1 | `OP_FRAMEBUFFER_REQUEST` (14) | gpud resolves the mode and answers with it and a clone of its scanout framebuffer object | `gpud: framebuffer granted (WxH)`, `windowd: display mode from gpud (WxH)` |
| 2 | `OP_GPU_CAPABILITIES` (16), `OP_GPU_CONTEXT_OPEN` (17) | windowd opens its GPU context with its pipeline library (built by `tools/nxgpu/`); QEMU's 2D device runs no GPU code and opens nothing | — |
| 3 | `OP_SET_FRAMEBUFFER_VMO` (3) | attach; carries no capability (one that does is refused) | — |
| 4 | `OP_REVEAL` (13) | the splash holds until the first present after this; that present's ack is `STATUS_REVEALED` | `display: first scanout ok` |

Without gpud windowd says `windowd: display none (…)` and runs display-less at the layout maximum. Mode: the device tree's request (`/chosen/nexus,display-mode`) against the device's capability (`resolve_display_mode`); the board uses the layout maximum, 1920x1080 at 60 Hz. EDID is parsed (`dc/edid.rs`), unused.

## Memory layout

`source/libs/nexus-display-proto/src/layout.rs` numbers rows at `LAYOUT_MAX` (1920x1080, BGRA, one stride):

| plane rows | content |
|---|---|
| 0 | wallpaper |
| 1 | retained plane (the last composed base) |
| 2–4 | `RING_ROWS`: the three scanout slots |
| 5… | atlas (8 planes by default; on the board sized from RAM by `gpu_share.rs`) |

On the board the ring is one contiguous object made for the controller (it cannot fault). Everything the GPU samples is one lazy object made for the GPU, committed when the GPU is told its pages (ADR-0078, `backend/dc/surfaces.rs`).

## A present

1. windowd sends `OP_PRESENT_DAMAGE`: `[op, seq u32]` + a serialized `nexus_gfx::CommittedBuffer` — `CompositeLayer` per layer back to front, `LayerTransform` per moving window or layer, `Damage`. At most two presents are in flight (`compositor/runtime/gpud.rs`). The ack window (`present_acks.rs`) credits outstanding seqs only. Before the encode, a window wholly under the opaque bodies above it is left out (`compositor/runtime/occlusion.rs`, RFC-0101 §8; `windowd: windows occluded (n=`) — on the GPU path only, where a glass back is drawn opaque.
2. gpud validates the commands (`nexus_gfx::command::validation`), executes them into the display and scans out (`present_chain.rs`). Hops: `G1 recv` → `G2 parse` → `G3 exec` → `G4 scanout`; the virgl path adds `G3b`/`G3c` ([gpud-ring.md](gpud-ring.md)).
3. The ack echoes the seq and status and carries `frame_time::Shown`: when the frame reached the glass, the refresh period, the cadence. windowd's `frame_clock.rs` predicts the next shown time from it; clients waiting on `OP_SURFACE_FRAME_REQ` get that time.
4. A present the virtio device lost (`present_lost`) is NACKed; windowd re-sends the damage.

App surfaces are composed by reference: windowd asks for a buffer per surface (`OP_SURFACE_ALLOC` 18, `OP_SURFACE_FREE` 19). Their pixels are straight-alpha BGRA (RFC-0109; app-host converts, `straight_alpha.rs`). gpud makes it for the sampling device and charges the owner (`surface_serve.rs`, `surfaces.rs`). On the board each surface is bound whole in windowd's GPU context; its binding table grows from the heap (`VmSpace`, ADR-0093) up to the layout's GiB (`surface_va.rs`); `gpud: bindings (in_use= of=)` counts them. A freed buffer closes after the last present that could read it retired. Across a compositor restart (RFC-0112 Phase 3) gpud keeps every object as an orphan with its last drawn place (`surfaces.rs: note_places`); the new windowd adopts the shown ones before its first frame (`OP_SURFACE_ADOPT` 20, `surface_adopt.rs`) as ghost windows until each app presents again; the rest are released.

## The three displays

| | QEMU 2D (`visible-2d`) | QEMU virgl (`visible`, `visible-fhd`) | board display controller |
|---|---|---|---|
| who draws | CPU executor `backend/cpu_frame.rs` over `nexus_gfx::raster` | GPU passes of the pipeline library on the GL scanout (`gl_scanout.rs`, `virgl_composite.rs`); CPU fallbacks per command | the GPU in windowd's context into a free ring slot (`backend/dc/gpu_frame.rs`); otherwise refused |
| to the glass | `TRANSFER_TO_HOST_2D` + flush | double-buffered swapchain (`gl_scanout_init.rs`); `SET_SCANOUT` + flush is the batch tail | flip through the controller's configuration latch (`backend/dc/flip.rs`) |
| ack | at once, 60 Hz period named | at once, 60 Hz period named | deferred until the flip landed (`vblank_wake`) |
| pointer | virtio cursor queue | procedural GL cursor | the controller's 64×64 pointer layer (`backend/dc/cursor.rs`); a move is no present |

## Board flips and cadence

- `backend/flip_ring.rs` runs the three ring slots: one scanned, one latched (taken at the next frame boundary), one free. A present drawn while a flip is latched waits drawn; a third finds no slot and is refused.
- A flip has landed when the latch reads clear, at the interrupt wake or the loop's next pass. Its time comes from the scan-line counter, not from the wake.
- `backend/frame_pace.rs` (RFC-0110) holds a cadence of one to three periods (`CADENCE_MAX`), chosen from the second longest of the last 8 draws. A frame drawn early is held to the boundary before its slot, so frames land evenly apart.
- The splash holds the glass until the first present after `OP_REVEAL` (`backend/splash_hold.rs`); a switch that did not take keeps it and acks no reveal.

Board markers: `gpud: dc vblank ok (`, `gpud: dc flip ok (`, `gpud: dc flips (… cadence= paced= late=)`, `gpud: dc reveal flip ok (`; `gpud: FAIL dc present front` is forbidden (tearing).

The gate lines are a series (ADR-0094): `last=` is the last recorded cycle's value (`nx ledger series add <uart.log>`, then `lint --fix`); 10 % worse is `FAIL … slower than last:` naming the cycle.

## Debugging a broken hop

Prove the first broken hop in the smallest place:

1. capability route and rights,
2. request/reply on the wire (`nexus_display_proto` host tests),
3. the owner's state transition,
4. the downstream output (the `gpud: chain G…` hops),
5. the observer marker.

A marker QEMU misses while host tests pass is a hop they do not cover.
