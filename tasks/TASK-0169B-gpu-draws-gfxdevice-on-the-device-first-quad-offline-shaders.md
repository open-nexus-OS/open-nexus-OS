---
title: TASK-0169B The GPU draws — `GfxDevice` on the device (G2) and the first textured quad from offline shaders (G3's first op) — Block 4 layer 4, RFC-0101 Phase 3
status: Done (2026-10-13)
owner: @gpu @compositor
created: 2026-04-10
recut: 2026-10-13 (from the v1b "resource/fence core + CPU mock" draft, whose host half landed as TASK-0280's `GfxDevice` v2 + `cpu_mock`)
depends-on: [TASK-0329 (G1, the device online), TASK-0280 (the v2 contract, host), TASK-0216 (the plane ring)]
follow-up-tasks: [TASK-0215 + TASK-0216 (layer 5: the desktop on the GPU — the render queue, blending, blur), TASK-0290 (M2: the uncached mapping for several commands in flight)]
links:
  - Contract: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§4, §7, §8c Phase 3)
  - Order: tasks/IMPLEMENTATION-ORDER.md ("Block 4 in layers", layer 4)
  - Measurement: docs/board/measurements/2026-10-13-gpu-first-quad/README.md
  - The device online (layer 3): tasks/TASK-0329-gpu-device-driver-img-b-series-gpud-backend-firmware-mmu-submission.md
  - The scanout contract (layer 2): tasks/TASK-0216-…
  - Method: .claude/skills/driver-bringup/SKILL.md
---

## Context

Layer 3 (TASK-0329, 2026-10-13) left the board's GPU online: the domain through socd, the
published firmware booted by the open start sequence, one address space bound and flushed, a
null job's completion bridged to the kernel fence 64 times at 131 µs a round trip (the stock
stack's fixed frame cost is 640 µs). Nothing has been drawn yet: `gpud` still composites on the
CPU into the plane ring of layer 2 and the GPU is a proven, idle peripheral.

This layer makes the GPU draw into a plane of the ring — and it does so **without a shader
compiler on the device**. RFC-0101 §7 says GPU code is client data: finished binaries at a bound
address with a declared target. The binaries come from the open userspace driver for this core
(the open toolchain knows BVNC 36.29.52.182 in its device table) run on the host over a shim
that stands in for the kernel: the driver's standard graphics API is driven with a clear and a
filtered, scaled blit of a 256×256 texture into a linear 1920×1080 target, and the shim's hook
writes **every bound buffer with its GPU address and both jobs' command streams** to disk. That
capture is the golden: the device binds the same bytes at the same addresses, points the target
binding at a plane of the ring, and submits the same two transfer jobs. The firmware-side form of
a transfer job is the open kernel driver's `cmd_transfer` (a 192-byte struct of 17 32-bit and 13
64-bit registers), filled from the stream by its stream definition — a pure, host-tested decoder.

What the capture showed (2026-10-13, `docs/board/measurements/2026-10-13-gpu-first-quad/`):
two transfer jobs on one transfer context (flags `SINGLE_CORE`, 176-byte streams: the
`frag_screen` word is present because the core has `GPU_MULTICORE_SUPPORT`, as our firmware
device-info goldens say too); the clear carries the colour in `usc_clear_register0`
(`0xff291f1a` = BGRA of 0.10/0.12/0.16/1.0) and no background program; the blit carries a PDS
background program (`pds_bgnd0/1_base`), a 20 KiB USC code segment, texture/sampler state words
and the tile walk (`isp_mtile_size`, `isp_render_origin`); 28 buffers are bound, of which 11 are
non-zero and the target (8.1 MiB) and the texture (256 KiB) are ours to supply. The open
driver's transfer path offers no source-over blend (its "blend" resolve is a multi-sample
average), and on this architecture blending is fragment-shader code in the render path — so the
**blended** quad belongs to layer 5's render queue, and this layer's op is the textured quad.

## Three lenses (architecture-review, before code)

- **Scope.** In: `userspace/nexus-gfx/src/backend/gpu/{fwif/transfer.rs,scene.rs}` (the transfer
  command, the stream decoder, the scene-bundle format) + tests + `examples/scene_pack.rs` (the
  packer from a capture directory); `resources/gpu/scenes/first-quad.scene` + `README.md`
  (provenance); `source/drivers/gpud/src/backend/gpu/{device_v2.rs,quad.rs}` (G2's `GfxDevice`
  over the G1 `Gpu`; the replay rung); `service.rs` (the rung after `gpu fence ok`); the proof
  manifest and the board ladder; RFC-0101 §8c + the phase row; this ledger; the measurement
  folder; ORDER; CHANGELOG. **Out (layer 5):** the render queue (geometry/fragment jobs, the
  render-target dataset, free lists), blending, blur, windowd's use of the device over IPC, and
  the retirement of the v1 trait + boolean fence (honest only when windowd stands on v2).
  **Out (M2):** the uncached mapping for several commands in flight (one job in flight here).
- **Invariant.** A submit references only memory bound in its context; the scene's every buffer
  is bound before the job, the target binding is exactly one plane of the ring, and a stream is
  accepted only in the exact feature-gated length with a zero padding word, as the open kernel
  driver does. The scene file is untrusted bytes: `test_reject_scene_for_another_core`,
  `test_reject_scene_run_outside_buffer`, `test_reject_scene_buffer_outside_heaps`,
  `test_reject_stream_length_for_features`, `test_reject_stream_padding_nonzero`,
  `test_reject_submit_references_unbound_va` (v2, kept). The device checks the scene's core id
  against its own (§7's declared target).
- **Contract.** RFC-0101 §4 (one batch, one crossing), §7 (GPU code is client data, target
  checked), **§8c Phase 3** (the transfer job's firmware form, the scene bundle, the gates). The
  SSOTs extended: `nexus_gfx::backend::gpu::fwif` (the firmware interface types), `scene` (new,
  the bundle), `gpud`'s backend (the first `GfxDevice` on hardware), the proof manifest.

## P0 — Measurement (host capture + hypotheses) ✅ 2026-10-13

`docs/board/measurements/2026-10-13-gpu-first-quad/README.md`: the capture recipe (the open
driver + shim in a volatile container, the hook, the program), the decoded streams of both
jobs, the buffer table, the five hypotheses with their decision lines.

## P1 — Model (host) ✅ 2026-10-13

- `fwif/transfer.rs`: `TransferRegs` / `TransferCmd` (192 B, offsets asserted against the open
  kernel driver's layout), `StreamFeatures::of(&DeviceInfoBlock)` (the device decides the gated
  words), `TransferCmd::from_stream(bytes, feats, single_core)`, `to_bytes()`; five tests
  including `test_reject_stream_length_for_features`, `test_reject_stream_padding_nonzero`,
  `test_reject_extension_stream`.
- `scene/{mod,pack}.rs`: the bundle (`NXSC` v1: buffers with sparse non-zero runs and a kind —
  data, target, texture — and transfer jobs with their streams), `Scene::parse(bytes, CoreId)`
  with the reject matrix (`test_reject_scene_for_another_core`, `_run_outside_buffer`,
  `_buffer_outside_heaps`, overlap, missing target, truncation, foreign header), the texture
  formula `checker_v1` (FNV-1a 64 `0x940b44898e744af5` = the captured texture's), the builder
  and `runs_of`; `examples/scene_pack.rs` packs a capture directory and refuses a capture whose
  buffers were rewritten between jobs.
- `resources/gpu/scenes/first-quad.scene` (35 056 bytes: 29 buffers, 33 031 data bytes as
  runs, 2 jobs; FNV-1a 64 `0x699ff414b2672125`) + `README.md` (provenance, repacking);
  `tests/gpu_scene_goldens.rs`: the digest, the counts, the target = one plane, both streams'
  registers as the measurement README lists them.
- v2: `Stream::Transfer { stream, single_core }` (the mock refuses it by name),
  `DeviceError::{CommandsUnsupported, CacheModeUnsupported, MalformedStream, Lost}`.

## P2 — First light (board) ✅ 2026-10-13 (cycle 1 green, `build/logs/board--2026-10-13T16-04-35`)

`gpud: gpu fill ok (us=1857 px=1920x1080 ns_per_px=0.8 at or below the stock 1.8 bind_us=4603)`,
`gpud: dc reveal flip ok (…)`, `gpud: gpu quad ok (us=1323 samples=4/4 shown row=2160)`;
`[PASS] board-visible: 57 rungs, 7 operator marker(s), no FAIL marker`. H1–H3 decided for, H4–H6
recorded (measurement README). The quad replaced the boot splash until windowd's first present.

- `device_v2.rs`: `GfxDevice` for the G1 `Gpu`: `create_bo` (a `DmaVmo`), `vm_bind` (the G1 page
  tables + the waited MMU-cache flush), `submit` (the transfer command + `UPDATE` into the
  transfer context's CCB, one kick, one IRQ, the timeline point), `reached`. The plane ring's VMO
  is bound by its `vmo_runs` at the scene's target address.
- `quad.rs`: the rung after `gpu fence ok`: bind the scene (zero buffers as zero pages, the texture
  generated), clean the plane (CPU → device ownership), submit job 0 (clear) → `gpud: gpu fill ok
  (us= px=1920x1080 ns_per_px=)`; submit job 1 (blit) → read back four sample pixels after an
  invalidate: the background at a corner, the white frame at the quad's edge, the two checker
  colours — `gpud: gpu quad ok (us= samples=4/4)`; the FAIL forms name the sample, expected and
  read value, or the FWCCB fault type and address. Then the plane is queued for scanout so the
  operator sees the quad (`[PASS] board-visible`).
- Markers REQUIRED in `scripts/board-test.sh`'s ladders and declared in the proof manifest.

## P3 — Gate sweep ✅ 2026-10-13

- The gate shown red: the lane refuses cycle 1's capture with the `gpu quad ok` line removed
  (`rung missing: 'gpud: gpu quad ok (' (rung 29 of 57)`, `cycle1-gate-red.txt`). The device-side
  FAIL form (sample expected/read, firmware fault) was not exercised — the first cycle was green;
  it fires on the next real failure.
- `just check` green (structure gate: `scene` split into `mod`/`pack`); `just test-all` green
  after the board cycle (72 PASS, the QEMU ladders unchanged — the GPU module is not granted there).
- Numbers: bind 4 603 µs (29 buffers, 28 flushes); fill 1 857 µs = 0.8 ns/px (stock 1.8); blit
  1 323 µs; fence round trip 109 µs this boot; firmware boot 1 085 µs; one MMU flush 9 683 µs
  (handed on with the boot-time variance).

## Hypotheses the first cycle decides

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The firmware takes a `TQ_3D` command in G1's transfer context exactly like the null job (same CCB, `UPDATE` after, one IRQ) | `gpu fill ok` with the corner pixel = clear colour; an FWCCB fault (type 112) whose address lies inside a scene range = our binding, outside = the stream |
| H2 | The open driver's streams and binaries for this BVNC run unchanged on the core | the four sample pixels; a wrong colour = PBE/texture state (format, stride), a hang = PDS/USC addresses |
| H3 | A full-plane fill costs at most the stock 1.8 ns/px (3.7 ms) | `ns_per_px=` in `gpu fill ok` |
| H4 | The CPU's cached plane lines must be cleaned before the GPU writes and invalidated before the readback | samples read the GPU's pixels only with both; "old CPU pixels" at a sample = the rule |
| H5 | The firmware's boot-time variance (1.1 / 23 / 10.9 ms in layer 3) is load on the CPU side, not the firmware | `boot_us` across this layer's cycles, noted; the trace groups are the next instrument if it stays wide |

## Definition of Done

1. ✅ The measurement folder exists and answers its question with archived raw data (the
   capture recipe and hook, both streams, the job headers, the decoded registers, the buffer
   table, cycle 1's lines).
2. ✅ The host model reproduces the captured streams (`gpu_scene_goldens`) and refuses the
   malformed ones (eight `test_reject_*`).
3. ✅ The board lane is green on its REQUIRED ladder with `gpu fill ok (ns_per_px=0.8)` and
   `gpu quad ok (samples=4/4)`, the operator acked the ladder (`[PASS] board-visible`, 57 rungs);
   no tolerated red was added.
4. ✅ The gate has been shown red (the capture without the rung refused by name, archived);
   the device-side FAIL form awaits its first real failure.
