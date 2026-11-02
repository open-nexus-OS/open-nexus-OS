<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0092: A surface is a buffer the app draws and the GPU reads — gpud allocates it for its owner, windowd composes by reference

- Status: Proposed (2026-10-31, TASK-0348 — package 1 of the operator's plan of 2026-10-30)
- Date: 2026-10-31
- Links:
  - Tasks: `tasks/TASK-0348-zero-copy-surfaces-a-compositor-without-fixed-tables-and-its-restart.md`
  - RFCs: `docs/rfcs/RFC-0114-surfaces-are-shared-buffers-the-app-draws-where-the-gpu-reads-the-compositor-composes-by-reference.md`
    (the contract), RFC-0109 (surface layers), RFC-0112 (the restart, amended), RFC-0101 (the
    planner)
  - Related ADRs: ADR-0042 (why blit — **superseded for app surfaces**), ADR-0078 (GPU memory is
    the one pool), ADR-0079 (cache maintenance), ADR-0075 (the compositor never waits)

## Context

windowd copies every damaged row of every app surface into its atlas with two syscalls per
row; on the board that is 20–138 ms of the compositor's frame path (cycle 79). ADR-0042 chose
the copy for v1 because the app's VMO could not be sampled by the GPU and windowd needed a
quiescent snapshot. Both premises are gone or never held: gpud can make memory the GPU samples
(ADR-0078) and maps it into any context; the ack-before-copy order means the snapshot was never
quiescent. ADR-0078 already named "one object per window surface" the end state.

## Decision

1. **gpud allocates every app surface** — one GPU-reachable object per surface, made for the
   surface's owner (the app's kernel id, given by windowd), charged to that owner, within the
   GPU share ceiling. gpud serves windowd only; the app receives the object's capability through
   windowd with its create ack and never a device capability.
2. **The object holds two slots** (three reserved for GPU-rendered apps), each the RFC-0109
   surface layout at a 256-byte-aligned stride. **The app draws into a slot and the GPU reads the
   same memory**; the app cleans the caches over what it drew (`vmo_clean`, syscall 74) before it
   presents; the kernel cleans frames it zeroes for a non-coherent device.
3. **The present ack is the release**: windowd acks a present when the surface's other slot is
   free — no longer its latest and read by no frame still in flight (gpud reports retirement).
4. **windowd composes by reference** — a layer names `(source, slot, rows)`; the source's
   generation, not a global epoch, decides uploads; a slot flip is never a backdrop change.
5. **windowd's tables grow with the windows**: stable `u16` window keys on every wire, growable
   per-window tables sized at open, no eviction of a live client; limits are memory's and the
   frame's layer budget's.

## Consequences

- **Positive:** the compositor's frame path copies nothing; no second resident copy of every
  window (the two slots cost what app VMO + atlas copy cost today); a fifth, tenth app window
  opens; the restart can keep every window's pixels (gpud holds the objects); direct scanout and
  GPU-rendered apps plug into the same object.
- **Negative / accepted cost:** a new syscall (74); a new gpud protocol pair (18/19); the
  surface wire v2 replaces v1 (no dual structure — every app-host speaks v2 in the same change);
  the app does a copy-forward of the last present's rects into its back slot; surface memory is
  committed at allocation on the board (bound whole, deterministic, no per-frame MMU flushes).
- **Follow-ups:** RFC-0115 (the owner's footprint includes its surfaces; jetsam), the layer-8
  package (direct scanout via `usage = SCANOUT`; apps rendering with the GPU into their slots).

## Alternatives considered

- **A cheaper copy** — still a copy on the frame path and a second resident image.
- **App-allocated memory imported by gpud** — the app cannot allocate inside the GPU's reach.
- **Slices of one pool object** — no slice capability, no revoke: a reused range would let one
  app write another's pixels.
- **Per-slot kernel fences** — 64 fences system-wide, not waitable with a channel; the ack the
  app already waits on carries the release.
