<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0078: GPU memory is the one pool's — committed when the GPU is told its pages, its share a fraction of RAM read at boot

- Status: Accepted
- Date: 2026-10-23
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6b-2b)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md`
    (§2, "One pool with the GPU"), `docs/rfcs/RFC-0101-…` (the GPU driver: contexts with their own
    GPU address space), `docs/rfcs/RFC-0098-…` C4 (memory made for a device, inside its reach)
  - Related ADRs: `docs/adr/0077-memory-is-committed-on-first-touch-the-kernel-touches-user-memory-only-through-a-checked-range.md`
    (lazy objects, the fault fills), `docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md`
  - Measurement: `docs/board/measurements/2026-10-23-layout-split/README.md`

## Context

gpud holds one display layout block for the whole session: 13 display planes, 107.8 MB at the
board's 1920x1080 — the wallpaper, the retained plane, the three ring slots the display controller
scans and an eight-plane surface atlas — one contiguous block, committed at bring-up whether a row
of it is drawn or not. That is a fixed GPU share in all but name, and it is both too large and too
small: board cycle 65 drew at most 3 676 of the atlas's 8 640 rows in a series that opened every
app, tiled, maximized and took a screenshot (28.2 MB of 66.4 MB), and yet eight planes is all the
window surfaces the machine can ever hold, on 4 GiB as on 320 MiB. The operator's direction
(2026-10-23): no fixed GPU memory — dynamic, a share of the machine like the desktop systems with
unified memory, the whole RAM usable.

Three facts constrain the answer. The display controller reads a plane by base and stride: it
cannot fault and has no address translation, so what it scans must be one contiguous run inside
its reach. The GPU reads through its own MMU, 4 KiB pages, one table per context, and has no
populate path either: a fault there resets the context. And the kernel already commits a lazy
object's page on first touch (ADR-0077); RFC-0100 §2 promised that a device's `vmo_runs` commits
the range it names, and the kernel still refused a lazy object for a device.

## Decision

1. **No partition.** GPU memory comes from the one frame pool, as ordinary objects. The only
   memory committed up front for a display is what its controller scans: the three ring slots, one
   contiguous object inside the controller's reach (24.9 MB at 1080p). Everything the GPU samples
   — the wallpaper, the retained plane, the surface atlas — is one lazy object made for the GPU.
2. **A page is committed when a device is told it.** A lazy object may be made for a device: its
   frames come from inside that device's reach. `vmo_runs` on it commits every page of the range it
   names before it answers (a device cannot fault); a page named to a device stays committed until
   the object dies (a purge, 6e, never takes one).
3. **The GPU enters a page when a frame first samples it.** gpud binds the lazy object into the
   client's GPU context as one sparse range: nothing is entered at bind. Before a pass, for every
   rectangle the frame samples, gpud enters the pages of that rectangle that are not yet entered —
   naming them commits them. The pages entered are computed from the same values the GPU reads
   (the descriptor's address, width, height and stride), so no draw reads an unentered page; an
   unentered page is left invalid rather than mapped to a shared zero page, so a wrong range is a
   named GPU fault, never quietly black texels. windowd's writes (`vmo_write`) and the GPU's
   entries name the same frames: the object's page table is the one truth.
4. **The share is a fraction of the machine, computed at boot.** gpud reads the RAM the kernel
   took from the device tree (`mm_stats`) and derives two numbers, no constant in between: the
   GPU's ceiling — **two thirds of RAM**, about what the desktop systems with unified memory report
   as a GPU's working set at these sizes — past which a GPU buffer is refused by name; and the
   surfaces' reservation — **an eighth of RAM** in whole display planes, at least the fixed layout,
   at most what the ceiling leaves — reserved, never committed until drawn. On the 4 GiB board:
   a ceiling of 2.65 GiB and 64 planes of surfaces (59 for the atlas, eight before); on QEMU's
   320 MiB the fixed layout. The policy lives in gpud until `memd` owns memory policy (6e).
5. **The grant names the size.** The framebuffer grant's length (an existing field) carries the
   surface object's bytes — whole display planes, at least the fixed layout; windowd sizes its
   atlas from it. The row formulas of `nexus-display-proto::layout` stay; the atlas ends where the
   object ends.

## Consequences

- The board's display memory is the ring plus what is drawn: the measured series needs ≈ 28 MB of
  atlas and the wallpaper where 107.8 MB were held, and the atlas can hold 59 planes of windows.
- The QEMU display keeps one eager object of the fixed layout: virtio attaches a resource's backing
  by listing every page (which would name, and so commit, all of it), and virgl aliases the atlas
  into one texture. A per-band attach is 6f's (`lowmem`), where QEMU's memory is the constraint.
- A lazy object's cost before its first touch is the kernel's flat page table, four bytes a page
  (0.5 MiB for the board's 512 MiB of surfaces, from an 8 MiB kernel heap); a reservation that
  outgrows it takes a two-level table. The GPU's own page-table pool (1 024 pages) bounds what can
  be entered at 4 KiB pages (~1.9 GB); sizing it from the ceiling comes with app surfaces of their
  own (layer 8, G5).
- Accounting: the surface object's committed pages count in gpud's RSS (gpud maps it to keep the
  caches coherent for the GPU) until M4 charges GPU memory to the client it serves (6e).
- `gpud: gpu memory (ceiling= surfaces= ring= of=)` at bring-up and `gpud: layout split ok (…)`
  after the first frame drawn from the split are the board's evidence.

## Alternatives considered

- **A fixed GPU share** (the 107.8 MB block, or any constant): rejected — the operator's direction,
  and the measured use is a quarter of it in a full series while the capacity it gives is fixed.
- **Binding the lazy object whole at open:** `vm_bind` names every page it binds, so it would
  commit all of it — no saving.
- **A shared zero page under every unentered GPU page:** hides a wrong range as black texels instead
  of naming it; rejected for the loud failure.
- **One object per window surface:** the end state (layer 8, G5: zero-copy app surfaces); the row
  contract stays until then.
