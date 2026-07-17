# ADR-0032: GPU Command Ring + Pipelined Present (gpud virtio-gpu)

- Status: Accepted
- Created: 2026-06-16
- RFC: `docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md`
- Supersedes the single-buffer serialized submit in `source/drivers/gpud/src/backend.rs`
- Related: ADR-0031 (three-layer animation), ADR-0028 (windowd present chain), ADR-0039 (device-class driver architecture — this ring is the GPU instance of the shared DriverKit submit ring)

## Context

`gpud`'s virtio control queue submitted **one command at a time**: write a single
shared command buffer + descriptor pair, notify, then **block** until the device
advanced the used-ring, classify the response, repeat. On the `GPU_MODE=virgl`
GL-compositor present this was fatal:

- **Texture-sampling stall.** Pinned by a `COMPOSITOR_STAGE` bisection: pure clear +
  SDF draws (gradient/shadow) complete in ~256 µs, but the first **texture-sampling**
  `SUBMIT_3D` (wallpaper/glass-blur) does not advance the used-ring for ~500 ms —
  QEMU's virglrenderer defers that completion until a later `QUEUE_NOTIFY`. With a
  per-command blocking wait, every present ate one 500 ms deadline **per sampling
  draw** → 1–3 s per present (the screen "worked" but at ~0.5 Hz). QEMU/Mesa logged
  no GL error; fences (`VIRTIO_GPU_FLAG_FENCE`) made it worse (response-protocol
  break + post-scanout hang) and were rejected.
- **Bump-allocator OOM.** `Submit3d` stored its dword stream in a `Vec<u32>` and
  `as_bytes()` allocated a fresh `Vec<u8>` — ≈16 heap allocations per present. gpud
  runs on a non-freeing bump allocator (ADR/notes: per-frame `Vec` leaks), so once the
  present rate rose, `alloc-fail` hit within ~90 presents.

## Decision

Replace the single-buffer serialized submit with a **real multi-entry virtio command
ring** and a **pipelined, enqueue-only present**.

### 1. Multi-entry ring (`CtrlQueue`)
- `RING_SLOTS = 16` command slots; `QUEUE_LEN = 32` descriptors (one cmd→resp pair per
  slot). Command pool = `RING_SLOTS` contiguous 4 KiB pages; response pool = one page
  of `RING_SLOTS × 256 B` sub-slots. A `RingSlot(u16)` newtype keeps slot / descriptor
  (`2*slot`) / in-flight-count from being confused in the pointer arithmetic.
- `enqueue_pair` / `enqueue_single` write a slot's buffers + descriptors, publish to
  the avail-ring, notify — **without waiting** — and mark the slot in-flight in a
  `busy: u32` bitmask.
- `harvest` walks new `used.ring` entries (`id / 2` → slot) and frees them. A slot is
  **never reused until its completion is harvested**, so a slot's buffers are never
  overwritten while QEMU may still read them (the safety invariant).
- `alloc_free_slot` harvests then finds a free slot; back-pressure (block on the GPU
  ring-buffer IRQ, deadline-bounded) only if the ring is full.
- `wait_slot` is the **synchronous** path (`submit_two`/`submit_no_response`): used by
  init + the 2D/mmio present, where each command's response must be in hand before the
  next. Behaviour there is byte-identical to the pre-ring code.

### 2. Pipelined present
- A `ctrl_batch` flag routes `ctrl_submit_*` to `enqueue_*` (no wait).
- `compositor_buildup_present` enqueues every `SUBMIT_3D` draw + the final flush, then
  `ctrl_batch_end` — which **harvests prior frames but never blocks on this one**.
  Frame N+1's enqueues (their `QUEUE_NOTIFY`s) drive frame N's deferred completion, so
  a textured draw whose completion QEMU defers no longer blocks the present.
- Hop markers `GPUD_CHAIN_BATCH_SUBMIT` (G3b, present enqueued) and
  `GPUD_CHAIN_BATCH_OK` (G3c, pipeline flowing) make the path visible on a real run.

### 3. Heap-free `Submit3d`
- `words: [u32; 1024]` inline (a control-queue command is ≤ 4096 B = 1024 dwords), and
  `as_bytes() -> &[u8]` is a zero-copy view (riscv64 is little-endian). Zero heap per
  draw — no leak.

### Rejected
- **`VIRTIO_GPU_FLAG_FENCE`**: QEMU's fenced completion doesn't integrate with the
  used-ring + response model here (UNKNOWN response + post-scanout hang). Reverted.
- **Drain at present-end (synchronous batch)**: an intermediate step — capped the stall
  at one 500 ms drain (vs N), but still intermittently blocked. Pipelining removed it.

## Consequences

- Present latency went from **1–3 s every frame** → **uniform 60–250 µs** across the
  whole run; no OOM (`0 alloc-fail`). mmio + virgl boot with 0 KPGF/PANIC/USER-PF and
  reach `chain G4 scanout ok`. The GPU ring-buffer IRQ stays the reactive completion
  source (`gpud: gpu irq wake`).
- The ring is correct for both modes: synchronous (`submit_two` = enqueue + `wait_slot`
  + classify) and pipelined (enqueue-only + `harvest`). mmio is unaffected in observable
  behaviour.
- **Known limitation — present cadence.** The gpud spin-demo self-paces via a recv
  **timeout** (`Wait::Timeout(8.33 ms)`), which cannot reach 120 Hz when gpud is the
  only runnable task (the degenerate-spin scheduler path resets the deadline every
  iteration; a syscall `Reschedule` with no runnable task never reaches the kmain idle
  loop). Measured ~3.6–12 Hz. The fix is a **timer cap on a dedicated endpoint** (the
  proven path windowd's 120 Hz pacer uses via `process_expired_timers`), not the
  recv-timeout path. The real windowd-driven UI present path is already 120 Hz-capable;
  only the synthetic self-pace is limited. See
  `docs/architecture/graphics/gpud-command-ring-and-present-pipeline.md`.

## Tests

- Host contract/chain (`tools/nx`): `chain_gpu_scanout.rs::chain_gpu_batched_present_hops_in_order`
  pins the `G3 → G3b → G3c → G4` hop order via `GpudContract::with_batched_present()`;
  the existing `chain_gpu_*` + `cli_contract` suites stay green.
- gpud host unit tests (`cargo test -p gpud`, incl. `virgl.rs` Submit3d byte-format
  golden tests) stay green.
- Boot proofs (`scripts/qemu-test.sh`, `GPU_MODE=virgl` and mmio): present-stats
  `gpud: present us` shows uniform low `max`; `0 alloc-fail`.

## Addendum (2026-07-07): honest present outcome + NACK requeue (P0.3)

The ring's degraded recovery (reset/abandon after `GPU_WAIT_DEADLINE_NS`)
deliberately returns success so the single-threaded loop can never wedge — but
that made a deadline-missed present indistinguishable from a shown frame. On a
REACTIVE compositor that is fatal: windowd books the frame as presented, no new
damage arrives, and the (partially) lost frame stays on screen forever — the
"black RT with a green marker chain" class.

- **gpud**: the `OP_PRESENT_DAMAGE` handler snapshots the ring-wide
  `IRQ_DEADLINE_EXPIRED_COUNT` around the whole present. A non-zero delta means
  commands were abandoned — including failures swallowed inside optional draws
  (`let _ =`) — so the reply status becomes `STATUS_DEVICE_ERROR` (NACK) and
  `gpud: FAIL present deadline (cmd=N)` is emitted (no-alloc formatter; the
  counter delta is the one seam all deadline paths share).
- **windowd**: `drain_gpud_replies` treats a ≥5-byte non-OK reply as a present
  NACK: the frame is completed in the flow-control sense (slot freed, seq
  advanced so the stall watchdog keeps tracking genuine no-reply hangs), but
  FULL-frame damage is requeued — after an abandoned batch the RT state is
  undefined, so a partial repaint could leave stale regions. Bounded: 8
  consecutive retries (`windowd: present retry n=`), then one loud
  `windowd: FAIL present retries exhausted (n=)`. A clean ack resets the
  budget. The client reset (route teardown) is reserved for protocol garbage.
- **Pacing**: `frames_in_flight() > 0` keeps windowd's 120 Hz pacer armed so a
  NACK arriving while idle is drained within a tick, not on the next input.
- **Display truth**: the one-shot scanout readback after the first clean G4
  emits `gpud: scanout sample ok` → `SELFTEST: display nonblack ok` (measured
  host-GPU readback, #98 discipline) or `gpud: FAIL scanout black`; the
  postflight ladder consumes it three-valued (ok/FAIL/SKIP for 2D boots).
