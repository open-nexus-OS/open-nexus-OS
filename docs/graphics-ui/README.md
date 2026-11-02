---
schema: 1
title: "Graphics and UI stack"
role: guide
subsystem: ui
sources: [source/services/windowd, source/drivers/gpud, source/services/app-host, userspace/apps/window-kit, source/services/imed, source/services/memd]
contracts: [RFC-0067, RFC-0108, RFC-0109, RFC-0110, RFC-0113, RFC-0114, ADR-0068, ADR-0069, ADR-0071, ADR-0072, ADR-0075, ADR-0076, ADR-0078, ADR-0083, ADR-0086, ADR-0090, ADR-0092]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Graphics and UI stack

> **Purpose** — which service owns which part of the UI, from a key press to a pixel on the glass, and which contract defines each hop.
> **Owns** — `source/services/windowd/`, `source/drivers/gpud/`, `source/services/app-host/`, `userspace/apps/window-kit/`, `source/services/imed/`, `source/services/memd/`
> **Contracts** — [RFC-0067](../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md), [RFC-0114](../rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md), [ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md), [ADR-0075](../adr/0075-the-compositor-never-waits-on-a-client.md) — full list below
> **Invariants** — windowd is a compositor service and draws no UI: window chrome and controls are widgets (`window-kit`, DSL apps). windowd never waits on a client. gpud and imed take identity from the kernel's `sender_service_id` and serve the privileged ops to windowd alone.
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
| memd (`source/services/memd/`) | memory pressure policy: picks the victim by band, orders kills through abilitymgr | ADR-0085, ADR-0086 |
| screencapd (`source/services/screencapd/`) | screenshots: crop, pointer, PNG, file through vfsd | RFC-0095, ADR-0071 |

## The frame path

1. At start windowd asks gpud for the framebuffer (`OP_FRAMEBUFFER_REQUEST`); gpud decides the mode and grants it. windowd opens its GPU context with a pipeline library built by `tools/nxgpu/` ([display-chain.md](display-chain.md)).
2. An app creates a surface (`OP_SURFACE_CREATE`). windowd asks gpud for its buffer (`OP_SURFACE_ALLOC`); gpud makes it for the app's kernel id and charges that app. windowd moves the capability to the app (`OP_SURFACE_BUFFERS`).
3. app-host draws into the free slot, cleans the caches over it (`vmo_clean`) and presents the slot by number (`OP_SURFACE_PRESENT`) — no pixel crosses a syscall (`source/services/app-host/src/probe/surface_mem.rs`).
4. windowd composes by reference: a layer names (buffer, slot, rows). It sends gpud one present (`OP_PRESENT_DAMAGE`, a serialized `nexus_gfx` command buffer of `CompositeLayer`, `LayerTransform` and `Damage` commands), at most two in flight (`compositor/runtime/gpud.rs`).
5. gpud's frame planner (`userspace/nexus-gfx/src/frame/`) turns the layer set into GPU passes (QEMU's 2D device: the CPU executor) and scans out. gpud's ack is windowd's frame clock; once no frame in flight reads the app's other slot, windowd acks the app's present, which releases that slot.

Scroll physics and hover run in the app; windowd composes the layers the app declared (ADR-0076). The boundary is written at the top of `source/services/windowd/src/compositor/mod.rs`; leftover UI code in windowd is listed in [../dev/ui/windowd-cleanup-map.md](../dev/ui/windowd-cleanup-map.md).

## Input and text

- Pointer: hidrawd → inputd (one accelerated step plus every report as a sample) → windowd (`OP_UPDATE_VISIBLE_STATE`). windowd applies one staged sample per frame (`source/services/windowd/src/input_stage.rs`) for the surface under the pointer.
- Keys: inputd translates layout sources (us, de, dead keys) itself and sends text and edit actions on the ordered key channel (`OP_PUSH_KEY_INPUT`). A full queue holds inputd; it never drops a key. windowd hands them to the focused window. Composition sources go through imed, behind windowd (ADR-0090).
- Caret: the app declares it (`OP_SURFACE_CARET`); windowd draws and blinks it as one layer, with no app wake.

## Lifecycle and memory

- windowd states what is seen (focused, visible, hidden, gone) to abilitymgr (`source/services/windowd/src/seen.rs`). abilitymgr decides. A window hidden longer than `FREEZE_GRACE_NS` (5 s, `source/services/abilitymgr/src/seen.rs`) is frozen through its parent (ADR-0083).
- The kernel tells memd its pressure level. memd forwards it to windowd, and windowd tells every app window (`OP_SURFACE_PRESSURE`; app-host raises `MemoryPressure`). memd picks the victim by band (foreground > visible > background > hidden > suspended), the longest there first; at `critical` it squeezes, then swaps a suspended app before a kill (ADR-0086–0088).
- GPU memory comes from the one frame pool. Its ceiling is two thirds of RAM (`source/drivers/gpud/src/gpu_share.rs`). gpud charges each client's GPU memory to memd (`OP_GPU_CHARGE`, `source/drivers/gpud/src/charges.rs`).

## Screen capture

screencapd begins a capture through windowd (`OP_SURFACE_CAPTURE`). windowd freezes the screen. gpud reads the frame back (`OP_READBACK`) into a frame object screencapd lent. screencapd crops it, encodes the PNG and has vfsd write it. The shell draws the capture UI (ADR-0071, `compositor/runtime/capture.rs`).

## Frame pacing and the latency contract

- Budgets live in one crate, `source/libs/nexus-ux-budgets/` (RFC-0108): 16.7 ms per frame, input to present 50 ms warn / 100 ms fail, app launch 300 ms warn / 1 s fail. The board's timing is the contract, not QEMU's.
- gpud acks a present with the time it reached the glass, the refresh period and the cadence it holds. windowd predicts the next shown time from that (`source/services/windowd/src/frame_clock.rs`). Animating clients ask for a frame pulse (`OP_SURFACE_FRAME_REQ`) and step their motion to that time (RFC-0110).
- windowd restarts: init respawns it, apps attach again (RFC-0112).

## Contracts that define the stack

- [RFC-0067](../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md) — windowd is a compositor service; the rasterizer is in nexus-gfx, UI is in userspace apps.
- [RFC-0109](../rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md) / [ADR-0076](../adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md) — surface layers: base, scroll, material and wash layers.
- [RFC-0114](../rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md) / [ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md) — gpud allocates each app surface for its owner; windowd composes by reference; its tables grow with the windows.
- [ADR-0075](../adr/0075-the-compositor-never-waits-on-a-client.md) — every send to a client is non-blocking, with a bounded outbox.
- [ADR-0068](../adr/0068-modal-semantics-in-the-dsl-runtime-windowd-keeps-one-routing-verb.md) — modals live in the DSL runtime; windowd keeps one routing verb.
- [ADR-0069](../adr/0069-tiling-chords-normalized-by-inputd-applied-by-windowd-ui-in-the-kit.md) — tiling: inputd normalizes chords, windowd applies geometry, the UI is in window-kit.
- [ADR-0071](../adr/0071-screen-capture-readback-in-gpud-freeze-in-windowd-ui-in-the-shell.md) — capture: readback in gpud, freeze in windowd, UI in the shell.
- [ADR-0072](../adr/0072-gpu-programs-wgsl-our-ir-our-backends-pipeline-library.md) — GPU programs: WGSL, lowered to our IR, per-target pipeline library.
- [ADR-0078](../adr/0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md) — GPU memory is the one pool's; only the scanout ring is committed up front.
- [ADR-0083](../adr/0083-the-compositor-states-what-is-seen-the-lifecycle-authority-decides-the-parent-orders.md) — windowd states what is seen; abilitymgr decides; the parent freezes.
- [ADR-0085](../adr/0085-memory-pressure-is-an-event-the-pool-tells-one-sink-its-level-from-watermarks-the-machine-sets-and-a-purgeable-page-goes-first.md) / [ADR-0086](../adr/0086-memd-is-the-one-memory-authority-it-hears-the-pool-learns-the-bands-orders-a-kill-through-the-lifecycle-authority-and-a-restorable-app-comes-back-where-it-was.md) — pressure is an event; memd is the one memory authority.
- [ADR-0090](../adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md) / [RFC-0113](../rfcs/RFC-0113-instant-input-every-event-counted-keys-never-lost-the-source-decides-the-route.md) — the input source decides the route; windowd orders focus and keys and owns the caret.
- [RFC-0108](../rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md) / [RFC-0110](../rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md) — latency budgets; frame timing and cadence.
