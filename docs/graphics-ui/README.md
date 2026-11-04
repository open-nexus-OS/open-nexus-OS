---
schema: 1
title: "Graphics and UI stack"
role: guide
subsystem: ui
sources: [source/services/windowd, source/drivers/gpud, source/services/app-host, userspace/apps/window-kit, source/services/imed, source/services/memd]
contracts: [RFC-0067, RFC-0108, RFC-0109, RFC-0110, RFC-0113, RFC-0114, ADR-0068, ADR-0069, ADR-0071, ADR-0072, ADR-0075, ADR-0076, ADR-0078, ADR-0083, ADR-0086, ADR-0090, ADR-0092]
verified: 2026-11-04
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Graphics and UI stack

> **Purpose** — which service owns which part of the UI, from a key press to a pixel on the glass, and which contract defines each hop.
> **Owns** — `source/services/windowd/`, `source/drivers/gpud/`, `source/services/app-host/`, `userspace/apps/window-kit/`, `source/services/imed/`, `source/services/memd/`
> **Contracts** — [RFC-0067](../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md), [RFC-0114](../rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md), [ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md), [ADR-0075](../adr/0075-the-compositor-never-waits-on-a-client.md) — full list below
> **Invariants** — windowd is a compositor service and draws no UI: chrome and controls are widgets (`window-kit`, DSL apps). windowd never waits on a client. gpud and imed take identity from the kernel's `sender_service_id` and serve privileged ops to windowd alone.
> **Read next** — [display-chain.md](display-chain.md), [gpud-ring.md](gpud-ring.md), [nexus-gfx.md](nexus-gfx.md), [animation-stack.md](animation-stack.md), [../dev/ui/overview.md](../dev/ui/overview.md), [../dev/dsl/overview.md](../dev/dsl/overview.md)

## The stack at a glance

```
hidrawd ──HID batch──► inputd ──pointer / keys──► windowd ──event channel──► app-host (DSL app)
                                                    │  ▲                         │
                         composition keys ► imed ───┘  │ present (slot n)        │ draws into its
                                                    │  └─────────────────────────┘ gpud-made buffer
                                                    ▼
                                      gpud ── present ack (frame clock) ──► windowd
                                        │
                                        ▼
                         scanout: virtio-gpu (QEMU) · display controller (board)
```

## Components

| component | owns | contract |
|---|---|---|
| windowd (`source/services/windowd/`) | client surfaces, layer tree, damage, z-order and focus, input routing, the present to gpud, the caret layer, window transitions | RFC-0067, RFC-0109, RFC-0114, ADR-0075, ADR-0076 |
| gpud (`source/drivers/gpud/`) | the display device and mode, scanout memory, GPU contexts, the command ring, app surface buffers, readback | RFC-0101, ADR-0032, ADR-0072, ADR-0078, ADR-0092 |
| app-host (`source/services/app-host/`) | one process per app: mounts a compiled `.nxir` program, runs layout, scroll physics, hover and DSL animation, draws into its surface buffer | RFC-0109, RFC-0114 |
| window-kit (`userspace/apps/window-kit/`) | the shared DSL component library for window chrome (`WinAppWindow`, `WinAppMenu`, `WinModal`, `WinAlert`, `WinToast`) | ADR-0068, ADR-0069 |
| DSL `.nx` apps (`userspace/apps/`) | app and shell UI (launcher, desktop-shell, settings, …) | see [../dev/dsl/](../dev/dsl/overview.md), [../dev/ui/](../dev/ui/overview.md) |
| hidrawd (`source/services/hidrawd/`) | HID ingress: virtio-input and USB HID boot interfaces, batched to inputd | RFC-0053 |
| inputd (`source/services/inputd/`) | pointer state in display space, motion samples, keymaps and dead keys, the key channel to windowd | RFC-0113, ADR-0090 |
| imed (`source/services/imed/`) | the composition filter for ja/ko/zh sources; windowd opens its session and sends its keys | RFC-0075, ADR-0090 |
| memd (`source/services/memd/`) | pressure policy: picks the victim by band, orders kills through abilitymgr; an owner's footprint includes its surfaces | ADR-0085, ADR-0086, ADR-0093 |
| screencapd (`source/services/screencapd/`) | screenshots: crop, pointer, PNG, file through vfsd | RFC-0095, ADR-0071 |

## The frame path

1. windowd asks gpud for the framebuffer (`OP_FRAMEBUFFER_REQUEST`); gpud decides the mode. windowd opens its GPU context with a pipeline library from `tools/nxgpu/` ([display-chain.md](display-chain.md)).
2. An app creates a surface (`OP_SURFACE_CREATE`); windowd asks gpud for its buffer (`OP_SURFACE_ALLOC`), made for the app's kernel id and charged to it, and moves the capability to the app (`OP_SURFACE_BUFFERS`).
3. app-host draws into the free slot, cleans the caches over it (`vmo_clean`) and presents it by number (`OP_SURFACE_PRESENT`); no pixel crosses a syscall (`source/services/app-host/src/probe/surface_mem.rs`).
4. windowd composes by reference — a layer names (buffer, slot, rows) — and sends gpud one present (`OP_PRESENT_DAMAGE`, a serialized `nexus_gfx` command buffer), at most two in flight (`compositor/runtime/gpud.rs`).
5. gpud's frame planner (`userspace/nexus-gfx/src/frame/`) turns the layers into GPU passes (QEMU's 2D device: the CPU executor) and scans out. Its ack is windowd's frame clock; an app's present is acked once no frame in flight reads its other slot.

Scroll physics and hover run in the app; windowd composes the declared layers (ADR-0076). The boundary is written atop `source/services/windowd/src/compositor/mod.rs`; leftover UI code in windowd: [../dev/ui/windowd-cleanup-map.md](../dev/ui/windowd-cleanup-map.md).

## Input and text

- Pointer: hidrawd → inputd (one accelerated step plus every report as a sample) → windowd (`OP_UPDATE_VISIBLE_STATE`), which applies one staged sample per frame (`source/services/windowd/src/input_stage.rs`) to the surface under the pointer.
- Keys: inputd translates layouts and sends text, edit actions and the registry's chords on the ordered key channel (`OP_PUSH_KEY_INPUT`; RFC-0113 §3); a full queue holds inputd, never drops a key. windowd hands keys to the focused window, applies chords; composition goes through imed (ADR-0090).
- Caret: the app declares it (`OP_SURFACE_CARET`); windowd draws and blinks it as one layer, no app wake.

## Lifecycle and memory

- windowd states what is seen (focused, visible, hidden, gone) to abilitymgr (`source/services/windowd/src/seen.rs`), which decides: a window hidden past `FREEZE_GRACE_NS` (5 s, `source/services/abilitymgr/src/seen.rs`) is frozen through its parent (ADR-0083).
- The kernel tells memd its pressure level; memd tells windowd, windowd every app window (`OP_SURFACE_PRESSURE`; app-host raises `MemoryPressure`). memd picks the victim by band (foreground > visible > background > hidden > suspended), longest first; at `critical` it squeezes, swaps, then kills (ADR-0086–0088).
- GPU memory comes from the one frame pool, its ceiling two thirds of RAM (`source/drivers/gpud/src/gpu_share.rs`); gpud charges each client to memd (`OP_GPU_CHARGE`, `source/drivers/gpud/src/charges.rs`). An owner's footprint includes its surfaces (ADR-0093): abilitymgr's band push names the owner (`app:<bundle id>`, memd wire v2); memd joins the charge to its processes. Limits per app: an owner's surfaces stop at the ceiling less the compositor's eighth (`gpud: FAIL surface alloc (… past its budget …)`); abilitymgr refuses an app's 65th instance by name (`abilitymgr: launch refused`).

## Screen capture

screencapd asks windowd (`OP_SURFACE_CAPTURE`), which freezes the screen; gpud reads the frame back (`OP_READBACK`) into screencapd's object; screencapd crops and encodes the PNG, vfsd writes it; the shell draws the UI (ADR-0071, `compositor/runtime/capture.rs`).

## Frame pacing and the latency contract

- Budgets live in `source/libs/nexus-ux-budgets/` (RFC-0108): 16.7 ms per frame, input to present 50 ms warn / 100 ms fail, app launch 300 ms warn / 1 s fail; the board's timing is the contract, not QEMU's.
- gpud's present ack carries glass time, refresh period and cadence; windowd predicts the next shown time (`source/services/windowd/src/frame_clock.rs`); animating clients ask for a pulse (`OP_SURFACE_FRAME_REQ`) and step to it (RFC-0110).
- windowd restarts: init respawns it; the new one adopts the apps' surfaces from gpud as ghosts (last pixels in place), apps rejoin and take them over; execd names the apps alive, so a dead app's ghost closes (RFC-0112).

## Contracts that define the stack

- [RFC-0067](../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md) — windowd is a compositor service; rasterizer in nexus-gfx, UI in apps.
- [RFC-0109](../rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md) / [ADR-0076](../adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md) — surface layers: base, scroll, material, wash.
- [RFC-0114](../rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md) / [ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md) — gpud allocates each app surface for its owner; windowd composes by reference; its tables grow with the windows.
- [ADR-0075](../adr/0075-the-compositor-never-waits-on-a-client.md) — every send to a client is non-blocking, the outbox bounded.
- [ADR-0068](../adr/0068-modal-semantics-in-the-dsl-runtime-windowd-keeps-one-routing-verb.md) — modals live in the DSL runtime; windowd keeps one routing verb.
- [ADR-0069](../adr/0069-tiling-chords-normalized-by-inputd-applied-by-windowd-ui-in-the-kit.md) — tiling: inputd normalizes chords, windowd applies geometry, UI in window-kit.
- [ADR-0071](../adr/0071-screen-capture-readback-in-gpud-freeze-in-windowd-ui-in-the-shell.md) — capture: readback in gpud, freeze in windowd, UI in the shell.
- [ADR-0072](../adr/0072-gpu-programs-wgsl-our-ir-our-backends-pipeline-library.md) — GPU programs: WGSL, our IR, per-target pipeline library.
- [ADR-0078](../adr/0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md) — GPU memory is the one pool's; only the scanout ring is committed up front.
- [ADR-0083](../adr/0083-the-compositor-states-what-is-seen-the-lifecycle-authority-decides-the-parent-orders.md) — windowd states what is seen; abilitymgr decides; the parent freezes.
- [ADR-0085](../adr/0085-memory-pressure-is-an-event-the-pool-tells-one-sink-its-level-from-watermarks-the-machine-sets-and-a-purgeable-page-goes-first.md) / [ADR-0086](../adr/0086-memd-is-the-one-memory-authority-it-hears-the-pool-learns-the-bands-orders-a-kill-through-the-lifecycle-authority-and-a-restorable-app-comes-back-where-it-was.md) — pressure is an event; memd is the one memory authority.
- [ADR-0090](../adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md) / [RFC-0113](../rfcs/RFC-0113-instant-input-every-event-counted-keys-never-lost-the-source-decides-the-route.md) — the input source decides the route; windowd orders focus and keys and owns the caret.
- [RFC-0108](../rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md) / [RFC-0110](../rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md) — latency budgets; frame timing and cadence.
