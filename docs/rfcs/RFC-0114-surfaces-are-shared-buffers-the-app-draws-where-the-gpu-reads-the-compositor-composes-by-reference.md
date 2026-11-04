---
schema: 1
id: RFC-0114
title: "Surfaces are shared buffers — the app draws where the GPU reads, the compositor composes by reference, and its tables grow with the windows"
status: in_progress
status_note: "seeded 2026-10-31 — package 1 of the operator's three-package plan, 2026-10-30"
owners: ["@ui", "@runtime", "@kernel-team"]
created: 2026-10-31
updated: 2026-11-02
subsystem: ui
tasks: [TASK-0348]
links:
  - label: ADRs
    target: "`docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md` (the decision), ADR-0042 (why blit — superseded for app surfaces), ADR-0078 (GPU memory is the one pool; \"one object per window surface: the end state\"), ADR-0079 (cache maintenance for non-drivers: \"a kernel call over an object the caller may write\"), ADR-0075 (the compositor never waits on a client), ADR-0091 (no ambient authority over address spaces)"
  - label: RFCs
    target: "RFC-0109 (surface layers: base, store, bands, washes), RFC-0101 (the GPU driver split; §8 the scanout contract and the planner's layer matching), RFC-0112 (the compositor's restart — amended by this package), RFC-0086 (the window feed), RFC-0108 (UX budgets)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0114: Surfaces are shared buffers — the app draws where the GPU reads, the compositor composes by reference, and its tables grow with the windows

> Status (migrated 2026-11-02): In Progress (seeded 2026-10-31 — package 1 of the operator's three-package plan, 2026-10-30; Phases 1–4 built and proven on the QEMU lanes 2026-11-02, the board cycle pending)

## Status at a Glance

- **Phase 0 (measured: what the copy and the fixed tables cost)**: 🟨 (board cycle 79 numbers in hand)
- **Phase 1 (the kernel: `vmo_clean`, clean zeroed device frames)**: ✅ (`KSELFTEST: vmo clean ok (`)
- **Phase 2 (gpud: surface objects for an owner, the by-reference layer, per-source generations)**: ✅ (QEMU 2D + virgl; the board's binding waits for its cycle)
- **Phase 3 (windowd composes by reference; the release protocol)**: ✅ (`windowd: compose copies=0 (` on headless, visible, usb-visible)
- **Phase 4 (app-host draws into its mapped slot; copy-forward; clean; present v2)**: ✅ (`apphost: surface mapped (`; typing, scroll, tile, resize, screenshot scenarios unchanged)
- **Phase 5 (windowd's tables grow with the windows: stable keys, no slot arrays)**: ✅ (`windowd: windows drawn (n=12)` on the many-windows lane)
- **Phase 6 (the restart, RFC-0112 amended: the screen survives with the apps' own pixels)**: 🟨
  (the restart and the rejoin built and QEMU-proven 2026-11-02 — RFC-0112 Phases 1–2; the
  ghosts with the apps' own pixels are RFC-0112 Phase 3, next)

Definition: "Complete" = no app pixel is copied by windowd on any lane (`windowd: compose
copies=0`), twelve app windows drawn in one frame (the many-windows lane), the release
protocol proven on the board (no tear, no stall), and windowd killed on the visible lane comes
back with every window's last pixels until its app presents again.

## Scope boundaries (anti-drift)

- **This RFC owns**: the surface buffer contract (who allocates, for whom, its layout and
  slots, how a write becomes visible to a non-coherent GPU, when a slot is free again), the
  app↔windowd surface protocol v2, the windowd↔gpud allocation protocol, the by-reference layer
  on the scene wire (source, slot, per-source generation), windowd's dynamic tables and stable
  window keys on every wire they cross.
- **This RFC does NOT own**: memory limits per app, jetsam, the large-memory grant (RFC-0115);
  direct scanout of a fullscreen surface and the GPU rendering path for apps (the layer-8
  package — this contract leaves room for both: a usage field in the allocation, the surface
  object bindable in any GPU context); the restart's fleet protocol (RFC-0112, amended here only
  where zero-copy changes it).

## Context

Measured on board cycle 79 (build/logs/board--2026-10-30T18-10-02):

1. **windowd copies every damaged app row** into its atlas: per row one `vmo_read` and one
   `vmo_write` (the app already paid one `vmo_write` per row to fill its own VMO) — up to
   4008 rows / 138 ms in one pass, often 600–850 rows / 20–24 ms. Those passes are windowd's
   frame path: the frame waits for them.
2. **The present ack precedes the copy.** windowd acks a present at dispatch and copies at the
   flush; the app may already repaint frame N+1's rows while frame N's are copied — ADR-0042's
   "the buffer is quiescent" does not hold. Zero-copy needs a real release point.
3. **One global content epoch**: any atlas write re-cleans every layer (board) and re-uploads
   every layer's rows (virgl).
4. **Fixed tables everywhere**: `MAX_APP_WINDOWS = 4` (a fifth app's surface is refused, the
   app exits), 8 event channels (a ninth evicts a LIVE app's channel — that app sees EOF and
   exits), 8 client outboxes, 8 window transforms in the GPU planner (window slot + 1 is the
   layer id on the wire; slot 15 + 1 collides with the desktop's owner 0x10), 8 feed entries.
5. **The kernel has no maintenance call for non-drivers** (ADR-0079 deferred it to its first
   user) and zeroes committed frames through the cacheable direct map without cleaning them:
   on the non-coherent board a GPU read of a page nobody wrote may return the frame's previous
   owner's data.

How the systems we measure against do it: macOS/iOS put window contents in IOSurfaces — the app
renders into the buffer, the compositor samples the same memory as a texture, no CPU copy, with
explicit double/triple buffering. Fuchsia's sysmem allocates one buffer collection all
participants (app, GPU, display) can use, by their declared constraints, and Flatland composes
images by reference with release fences. Both allocate in the service that knows the device's
constraints and hand the client a capability to the memory.

## Goals

1. **No app pixel is copied by the compositor.** The app draws into memory the GPU samples.
2. **One allocation authority for GPU-visible memory**: gpud, from the one pool (ADR-0078),
   charged to the surface's owner (its kernel service id), within the GPU share ceiling.
3. **A slot is reused only when no frame can still read it** — no tear, and never a stall of
   the compositor on a client (ADR-0075).
4. **windowd's tables grow with the windows.** Limits are memory's (memd, RFC-0115) and the
   per-frame layer budget's (fair share), never a fixed slot count.
5. **Room for the endgame**: direct scanout of a fullscreen surface, apps rendering with the GPU
   into their own surfaces, the compositor's restart keeping every window's last pixels.

## Non-Goals

- A general sysmem-style constraint solver for arbitrary devices (one GPU, one display
  controller; their constraints are gpud's).
- Changing the store/band model of RFC-0109 (bands, washes, the patch): it moves into the slot
  unchanged.
- Video/camera buffers, protected memory.

## Constraints / invariants (hard requirements)

- **Identity from the kernel**: a surface's owner is the kernel sender id of the app's create,
  never a payload field; gpud accepts allocation only from windowd's kernel id.
- **No device authority leaks**: the app gets a capability to the surface object (MAP), never a
  device capability, never a bus address.
- **The frame path allocates nothing and maps nothing**: allocation, GPU binding and MMU flushes
  happen at surface creation (and resize), never per frame.
- **Bounded**: slots per surface ≤ 3; damage rects per present ≤ 4 (RFC-0109); surface objects
  per owner and the GPU share ceiling enforced by gpud; the scene command buffer stays within
  `MAX_SERIALIZED`.
- **No fake green**: `windowd: compose copies=0` only when the frame referenced no copied row.

## Proposed design

### 1. The surface object (normative)

- One VMO per surface, made by gpud **for the GPU device** (its frames inside the GPU's DMA
  reach), holding `slots` (2; 3 reserved for GPU-rendered apps) **slots** stacked by rows.
- A slot is the surface layout of RFC-0109 unchanged: base rows `[0, height)`, store rows
  `[height, rows)`, at a **stride** of `round_up(width × 4, 256)` bytes. Slot `s` begins at row
  `s × rows` of the object. Pixel `(x, y)` of slot `s` is at byte
  `(s × rows + y) × stride + x × 4`.
- The object's kind is the backend's (sysmem's lesson — the consumer's constraints decide):
  - **board** (IMG GPU behind its MMU, non-coherent): lazy, bound read-only into windowd's GPU
    context **whole, at allocation** (one bind, one MMU flush), at a VA from gpud's GPU-VA
    allocator; no per-frame entering.
  - **virgl** (QEMU): eager (few runs), one 3D resource per object over the object's runs; a
    buffer layer whose source, slot or generation changed since it last drew transfers the rows
    it shows (the slot it now reads was last uploaded two presents ago — a damage-only transfer
    would miss the rects copied forward into it), waited before the present's ack.
  - **CPU 2D** (QEMU mmio lanes): eager, mapped read-only into gpud once.
- A resize is a destroy and a create (as before): a new object of the new size class, made at
  creation — never per frame. The replaced surface keeps showing its last frame **with the
  layers it declared** until its successor presents (a resize never shows a half-drawn window),
  then its object is freed. (Amended 2026-11-02: reusing a fitting object would need the app to
  keep a mapping across a destroy whose answer it cannot know in advance; the size classes keep
  gpud's allocator from fragmenting instead.)

### 2. windowd ↔ gpud: allocation (normative)

- `OP_SURFACE_ALLOC = 18` (windowd → gpud): `{owner_sid u64, width u16, rows u16, slots u8,
  usage u8}` — `usage` bit 0 = SAMPLE (always), bit 1 = SCANOUT (reserved: direct scanout).
  Reply: `{status, buffer u32, stride u32, alloc_width u16, alloc_rows u16, cache_block u16}` and
  the object's capability (MAP) moved on gpud's response route. `buffer` is gpud's id of the
  object within windowd's context (never 0; 0 names the layout object of today).
- `OP_SURFACE_FREE = 19` (windowd → gpud): `{buffer}` — gpud unbinds once the last frame that
  referenced it retired, closes its own capability and ends the charge. The object's memory
  lives on while the app still maps it (the kernel's lifetime rule).
- Only windowd's kernel id is served (as `OP_GPU_CONTEXT_OPEN` today). gpud charges
  `Part::Surfaces` to `owner_sid` (memd sees it in the owner's footprint, RFC-0115) and refuses
  past the GPU share ceiling with `QUOTA`.

### 3. app ↔ windowd: the surface protocol v2 (normative)

- `OP_SURFACE_CREATE` v2: the app asks (`width, height, rows, store_w, style, level, mode,
  resizable, nonce`) and sends **no** VMO (one that rides along is closed, the create refused).
  windowd allocates through gpud — never waiting on it — and answers on the app's event channel
  with `OP_SURFACE_BUFFERS = 38`: `{status, surface_id, stride, width (the class), rows_per_slot,
  slots, cache_block}` and the object's capability moved with the frame (in order behind what
  waited for the app: the outbox carries the capability). The app maps it read-write. A refusal
  (`QUOTA`: no memory for it) carries no capability; an app whose plan had a store asks once
  more without one (its scroll layers flat).
- `OP_SURFACE_PRESENT` v2: `{surface_id, seq, slot u8, count, rects ≤4}`. The rects are the
  slot's changed rows and columns (base and store coordinates as RFC-0109).
- **The present ack is the release**: windowd acks present `n` (slot `s`) when every other slot
  of the surface is free — i.e. the app may draw `n+1`. An invalid present (bad seq, slot,
  rect) is NACKed at once. Flow control stays "one present in flight" (today's), now meaning
  "one slot ahead".
- `OP_SURFACE_CLOSED` (windowd → app, ordered before windowd drops the app's channel): a window
  closed on purpose. An EOF without it is the compositor's death (RFC-0112 amended).

### 4. Writes the GPU can see (normative)

- `vmo_clean` — **syscall 74**: `(vmo_slot, offset, len)` cleans the CPU caches over that range
  of an object the caller holds **writable** (`Vmo`, not `VmoRo`; not sealed), bounded by the
  capability's length. A no-op returning 0 when the object was not made for a non-coherent
  device. Phased: the range's committed pieces are snapshotted under the kernel lock and cleaned
  with it dropped. This is ADR-0079's "kernel maintenance call over an object the caller may
  write"; user-mode `cbo` stays a driver's right.
- **A committed frame of an object made for a non-coherent device is clean before anyone can
  read it**: the kernel cleans it after zeroing (lazy commit and eager create).
- The app paints, cleans the rows it painted (`vmo_clean` per damaged row span), then presents.

### 5. When a slot is free (normative)

- gpud acks a present only once its GPU reads have finished (board: its flip landed after its
  last pass; virgl: its transfers waited; CPU: the present returned) — so a frame is **retired**
  when windowd holds its ack. windowd's retired floor is the newest frame with no older frame
  still in flight (an ack out of order retires nothing past the oldest outstanding; a lost gpud
  retires everything). No field is added to the ack.
- windowd records, per frame, the slots it referenced (at the send, with the frame's seq). A
  slot is **free** when it is not its surface's latest presented slot and every frame that
  referenced it has retired. windowd sends present `n`'s ack when the surface's other slots are
  free — at once when they already are, else on the gpud ack that frees them — and names the
  wait on its loop line (`work(released= held_us_max=)`).
- A surface its client destroyed owes no ack; a present to it is refused (`BAD_SURFACE`) — it is
  stale, not a fault.

### 6. Composing by reference (normative)

- `CompositeLayer` gains a 21st word, `source` (0 = the layout object — wallpaper, retained
  plane, windowd's own atlas patches; else a gpud `buffer`). `flags` bits 8–15 carry the slot.
  For `source ≠ 0`, `src_row`/`scroll_band_top` are slot-relative rows and `src_x` a column of
  the slot.
- `content_epoch` becomes the **source's generation** (the surface's present sequence for a
  buffer): the planner decides uploads (virgl) per source, not per frame; a write to windowd's
  atlas no longer touches app layers.
- The glass key and the planner's same-drawing match fold the source's identity and
  slot-relative rows, **never the slot**: a slot flip is not a backdrop change; changed pixels
  reach a glass through damage (RFC-0109's rule).
- The board's texture descriptor takes the source's base VA and stride (the hardware state
  already supports any stride < 2^15).
- windowd stops copying app rows and stops allocating atlas bands for app surfaces; the atlas
  keeps windowd's own content (the caret patch, solid and starting surfaces) and shrinks.

### 7. windowd's tables (normative)

- **Window keys** (`window_keys.rs`): a `u16` per window, `1..=0xFFFE`, climbing; a key a live
  window holds — its surface shown or still retiring, or a launch's starting surface — is
  skipped, so a key comes back only after every other one was handed out, long after its
  window's last frame retired. The desktop is `0xFFFF`, `0` names no window. The key is the layer
  id, the layer key's owner, the damage origin and the transform id on the scene wire. The search
  is bounded by the key space (none when every key is live), never a spin.
- **The slots grow**: a new window takes a free slot (no surface, none retiring, no launch
  waiting in it) or a new one registered in the z-stack. Every per-window table — the stack, the
  presentation tracker, the sightings, the feed's scratch, the event channels, the client
  outboxes — is a growable collection; the frame path reuses its scratch (cleared, never
  reallocated past warm-up). No live event channel is evicted, no client goes without an outbox.
- **Placement**: a window new to its slot takes the slot's place in a cascade computed from the
  room its frame leaves on the work area (`cascade_origin`): runs stepping down-right, centred
  in the room, each next run half a step right — any number of windows of any size, none off
  the area, none at another's point while the room's width allows.
- gpud's planner keys transforms by window key; a present with layers carries every live
  window's transform and replaces the table (bounded by the frame's layers: a drawn window
  spends at least one).
- The window feed (RFC-0086) carries up to 29 windows per frame (the IPC payload; a cut is
  named, `windowd: window feed bound (listed= open=)`) and the capture list up to the frame's 64
  layers.
- **Limits**: the app's memory (memd; TASK-0349) and the frame's layer budget (`MAX_LAYERS` =
  64: the desktop's base and bar and every drawn window's body first, the declared layers
  fair-shared — a frame draws the topmost 62 windows; a minimized window spends none, an occluded
  one still spends its body until occlusion culling, TASK-0350). gpud's board buffer table grows
  with the surfaces; a board context's binding table (the driverkit's `VmSpace`) starts with
  room for 128 ranges and grows from the heap past it (TASK-0351 P2, ADR-0093); the surfaces'
  addresses run from the private GiB's tail to the layout's GiB.
- **Proof**: `windowd: windows drawn (n=N)` — a new high of app windows that one frame, once it
  left for gpud, drew from their own buffers.

### Phases / milestones (contract-level)

- **Phase 0**: the board's copy cost and fixed-table refusals measured (cycle 79 in hand; a
  sixth-app refusal on the visible lane recorded).
- **Phase 1**: `vmo_clean` (74) + zeroed device frames clean — host tests and
  `KSELFTEST: vmo clean ok (`.
- **Phase 2**: gpud's surface objects on all three backends + the by-reference layer and
  per-source generations in nexus-gfx — host tests (planner, wire, glass key stable across a
  slot flip, descriptor with a stride ≠ 1920).
- **Phase 3**: windowd composes by reference with the release rule — host test of the release
  state machine; `windowd: compose copies=0` on headless and visible lanes.
- **Phase 4**: app-host paints into its mapped slot, copy-forward of the last present's rects,
  `vmo_clean`, present v2 — lanes green, typing/scroll scenarios unchanged.
- **Phase 5**: dynamic tables — twelve windows opened from the launcher on the many-windows lane
  (usb-visible's desktop on a 1 GiB guest), all twelve drawn in one frame
  (`windowd: windows drawn (n=12)`).
- **Phase 6**: the restart (RFC-0112 as amended).

## Security considerations

- **Threat model**: an app forging another app's buffer (a `buffer` id is gpud's, valid only
  in windowd's context; the app names its own surface by `surface_id`, which windowd maps to its
  owner); an app writing a buffer the GPU still reads (its own pixels only — at worst its own
  window tears); a stale capability after a window closed (the app keeps writing memory no frame
  samples; the memory stays charged until it closes); cross-process data through uncleaned
  frames (the kernel cleans zeroed frames of non-coherent device objects); flushing shared
  read-only pages for a cache side channel (`vmo_clean` requires a writable object).
- **Mitigations**: kernel identity for the owner; gpud serves windowd only; the app never holds
  a device capability; `vmo_clean` bounded by the capability, refused on `VmoRo`/sealed objects;
  allocation bounded by the GPU share ceiling and per-owner limits.
- **`test_reject_*`**: `vmo_clean` on a read-only alias, a sealed object, past the end;
  `OP_SURFACE_ALLOC` from a stranger; a present with a slot ≥ slots, a rect past the rows, a
  buffer id of another context; a window key collision.

## Failure model (normative)

- Allocation refused (`QUOTA`, ceiling or memory pressure) → the create is NACKed `QUOTA`; the
  app keeps its process and retries on the next pressure-normal (RFC-0115), never exits on it.
- `vmo_clean` errors are the caller's (EPERM on a read-only object, EINVAL past the end).
- gpud dies → windowd's frame path already handles a lost gpud (today's contract); surfaces are
  re-allocated when it is back.
- A slot never released (gpud never retires a frame) → the app does not draw; windowd's frame
  watchdog (RFC-0108) names it; no silent fallback to copying.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-gfx frame_plan frame_board buffer_wire
cd /home/jenning/open-nexus-OS && cargo test -p windowd surface_release window_keys cascade
cd /home/jenning/open-nexus-OS && cargo test -p nexus-display-proto client_surface
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os                 # CPU 2D path
cd /home/jenning/open-nexus-OS && just test-os visible         # virgl
cd /home/jenning/open-nexus-OS && just ci-os-usb-visible       # input + surfaces
cd /home/jenning/open-nexus-OS && just ci-os-many-windows      # twelve windows, one frame
```

### Deterministic markers

- `KSELFTEST: vmo clean ok (`
- `gpud: surface alloc ok (` · `windowd: compose copies=0` · `apphost: surface mapped (`
- `windowd: windows drawn (n=12)` (many-windows)

## Alternatives considered

- **Keep the copy, make it cheaper** (one syscall per rect instead of two per row): still a CPU
  copy on the compositor's frame path and a second resident copy of every window.
- **Slices of one big pool object per app**: one GPU binding for all, but the kernel has no
  slice capability and no revoke; a freed range reused while an old mapping lives would let one
  app write another's pixels.
- **The app allocates and gpud imports**: the app cannot make memory inside the GPU's reach
  (no device capability) — the allocation belongs where the constraints are known (sysmem).
- **A kernel fence per slot for the release**: 64 fences system-wide, no fence in a waitset;
  the release rides the ack the app already waits for.
- **A single-buffered store**: the app rewrites ring rows the active tree still declares
  (fills on a full ring, restarts after a jump, in-place redraws) before the present that
  re-declares them — the copy hides that today; without it the store tears.

## Open questions

- The board GPU's stride alignment: only 1920 px has ever run; 256-byte strides are assumed
  safe (Phase 2 measures one window at a non-1920 stride before the rest).
- Whether triple buffering pays for CPU-drawn apps (measured in Phase 4; reserved for GPU apps).
