# gpud command ring + present pipeline

How `gpud` submits work to QEMU's `virtio-gpu` device and how the virgl GL
compositor present is paced. Decision record: **ADR-0032**. RFC:
`RFC-0063`. Source: `source/drivers/gpud/src/{backend.rs,gl_scanout.rs,virgl.rs,markers.rs}`.

## Why this exists

The original control queue submitted **one command at a time** (single shared
command buffer; notify; block on the used-ring; classify; repeat). On the virgl
GL-compositor present that produced two failures:

1. **Texture-sampling stall.** A `SUBMIT_3D` that samples a texture (wallpaper /
   glass blur) is not completed by QEMU's virglrenderer until a *later*
   `QUEUE_NOTIFY` — its used-ring entry lags ~500 ms. With a per-command blocking
   wait, every present ate one deadline per sampling draw (1–3 s/frame). Pure
   clear + SDF draws complete in ~256 µs, so a `COMPOSITOR_STAGE` bisection pinned
   the staller to texture sampling. (No GL error; fences made it worse — rejected.)
2. **Bump-allocator OOM.** `Submit3d` was `Vec<u32>` + per-call `as_bytes()`
   allocation (~16 heap allocs/present) on gpud's non-freeing bump allocator →
   `alloc-fail` once the present rate rose.

## The ring (`CtrlQueue`, `backend.rs`)

```
RING_SLOTS = 16 command slots      QUEUE_LEN = 32 descriptors (one cmd→resp pair / slot)
cmd pool  = 16 × 4 KiB pages (contiguous)     resp pool = 1 page × (16 × 256 B sub-slots)
busy: u32 bitmask                  last_used: u16 (harvest cursor into used.ring)
```

- **`RingSlot(u16)`** newtype — `head_desc() = 2*slot`, `resp_desc() = 2*slot+1`.
  Keeps slot / descriptor index / in-flight count from being mixed in the unsafe
  pointer + descriptor arithmetic.
- **`enqueue_pair` / `enqueue_single`** — write a slot's command buffer + descriptor
  chain, publish to the avail-ring, `QUEUE_NOTIFY`, set the slot's `busy` bit. **No
  wait.**
- **`harvest`** — walk new `used.ring` entries; each element's `id / 2` is the slot
  that completed → clear its `busy` bit, advance `last_used`. The consumer half.
- **`alloc_free_slot`** — harvest, then take a free slot; if the ring is full,
  back-pressure (block on the GPU ring-buffer IRQ, deadline-bounded).
- **`wait_slot`** — synchronous single-slot wait (harvest + reactive IRQ block).
- **Safety invariant:** a slot is reused only after its completion is harvested, so
  its buffers are never overwritten while QEMU may still read them.

`CtrlQueue` holds raw pointers into device-shared memory and is intentionally
**`!Send`/`!Sync`** (single cooperative gpud thread; no `unsafe impl`).

## Two submit modes

| Caller | Path | Waits? |
|---|---|---|
| init, 2D / mmio present, every non-batched command (`submit_two`, `submit_no_response`) | `enqueue_* → wait_slot → classify_resp` | yes (synchronous, byte-identical to pre-ring) |
| virgl GL compositor present (`compositor_buildup_present`) | `ctrl_batch_begin → enqueue every draw + flush → ctrl_batch_end` | **no** (pipelined) |

### Pipelined present
`ctrl_batch` routes `ctrl_submit_*` to `enqueue_*`. The present enqueues all
`SUBMIT_3D` draws + the final `RESOURCE_FLUSH`, then `ctrl_batch_end` **harvests
prior frames but never blocks on this one**. Frame N+1's enqueues (their notifies)
drive frame N's deferred completion. A textured draw whose completion QEMU defers
therefore no longer blocks the present.

### Heap-free `Submit3d` (`virgl.rs`)
`words: [u32; 1024]` inline (a command is ≤ 4096 B = 1024 dwords); `as_bytes()` is a
zero-copy `&[u8]` view (riscv64 is little-endian). Zero heap per draw.

## Hop markers (`markers.rs`) — how far a real run gets

```
G1 recv present-damage → G2 parse ok → G3 exec ok
   → G3b batch submit ok (present enqueued)   ← GPUD_CHAIN_BATCH_SUBMIT
   → G4 scanout ok (frame presented)
   → G3c batch complete (drained)             ← GPUD_CHAIN_BATCH_OK (pipeline flowing, next frame)
```
`gpud: gpu irq wake` proves the reactive GPU ring-buffer IRQ is the completion source.

## Result

Present latency: **1–3 s every frame → uniform 60–250 µs** across the whole run;
`0 alloc-fail`; mmio + virgl boot with 0 KPGF/PANIC/USER-PF and reach G4.

## Limitation — present cadence (not yet 120 Hz)

The gpud spin-demo self-paces with a recv **timeout** (`Wait::Timeout(8.33 ms)`),
which cannot reach 120 Hz when gpud is the only runnable task: the degenerate-spin
scheduler path resets the deadline every iteration, and a syscall `Reschedule` with
no runnable task never reaches the kmain idle loop. Measured ~3.6–12 Hz. **Fix
direction:** a **timer cap on a dedicated endpoint** — the proven path windowd's
120 Hz pacer uses (`timer_create`/`timer_set` + `OP_TIMER_FIRED` via
`process_expired_timers`, which re-arms the SBI timer to the earliest deadline), not
the recv-timeout path. The real windowd-driven UI present path is already 120 Hz-
capable; only the synthetic self-pace is limited.

## Tests

- `tools/nx/tests/chain_gpu_scanout.rs::chain_gpu_batched_present_hops_in_order` —
  pins `G3 → G3b → G3c → G4` via `GpudContract::with_batched_present()` (host
  contract/chain simulation; `cargo test -p nx`).
- `cargo test -p gpud` — Submit3d byte-format golden tests + protocol size checks.
- `scripts/qemu-test.sh` (`GPU_MODE=virgl` + mmio) — boot proof: `gpud: present us`
  uniform low `max`, `0 alloc-fail`, hop ladder present.
