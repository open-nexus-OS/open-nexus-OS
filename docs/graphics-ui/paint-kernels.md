---
schema: 1
title: "The painter's kernels"
role: guide
subsystem: ui
sources: [userspace/ui/paint-kernels, userspace/ui/scene_raster/src/shapes.rs, source/services/app-host/src/probe/boot_env.rs, source/services/app-host/src/probe/paint.rs, source/apps/selftest-client/src/os_lite/probes/paintkernels.rs]
contracts: [ADR-0099, RFC-0119]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The painter's kernels

> **Purpose** — how the flat pixels of a frame are painted with the vector unit: which loops are kernels, how a program chooses their form, what proves a vector form right, and how the next kernel is added.
> **Owns** — `userspace/ui/paint-kernels/` (`reference.rs` the scalar forms, `riscv.rs` the vector forms, `proof.rs` the proof every machine runs, `lib.rs` the choice)
> **Contracts** — [ADR-0099](../adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md) rule 4, [RFC-0119](../rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md) (the kernel's answer, a thread's vector registers)
> **Invariants** — a vector form leaves the bytes its scalar reference leaves, for every input; no program is built with the vector extension — the vector instructions of a program are the ones in `riscv.rs`; a vector form runs only after the program stated the kernel's answer; no vector form names a register width.
> **Read next** — [README.md](README.md), [../kernel/float-state.md](../kernel/float-state.md)

## What a kernel is

A kernel is a loop a frame spends its pixels in, as one function of a span of a row:

| Kernel | What it does to every whole pixel of the span | Who calls it |
|---|---|---|
| `fill(row, px)` | the pixel becomes `px` | the row painter's replace span (`set_span`: a glass reset) and its opaque spans; app-host's row grounds (the page base, a motion layer's base, a band's ground) |
| `blend(row, bgra)` | the straight colour laid over it, source over: each channel `(dst · (255 − a) + colour · a) / 255`, the alpha channel with colour 255 | the row painter's translucent spans (`blend_span`: a box's interior between its corners, a border's straight run) |

Apps, motion layers and animations are painted row by row through `nexus-scene-raster`, so
they reach the kernels without knowing them. What a row computes per pixel — a corner's
coverage, a shadow, a gradient, a glyph — is not a kernel of this crate.

## Two forms, one result

- **The scalar reference** (`reference.rs`) is what a kernel *is*. It is plain Rust, runs on
  every machine and is what the host goldens are painted with.
- **The vector form** (`riscv.rs`) is assembly for the RISC-V vector extension. It takes its
  step from `vsetvli` — how many bytes a register group holds on this hart — so the same code
  runs on 16-byte registers (the emulator), 32-byte ones (the board) and any other width. It
  moves bytes, so a row needs no alignment, and it asks `vsetvli` for at most one group at a
  time: a step is then exactly what was asked, a multiple of four bytes, and the colour's
  channels stay on the pixels' channels.
- The blend works in 16-bit lanes: with `inv = 255 − a` no sum `dst · inv + colour · a` passes
  `255 · 255`, and the division by 255 is the high half of a multiply by `0x8081` shifted
  right by seven — exact for every 16-bit value (host-tested over all of them).

## How a program chooses

```rust
let width = nexus_abi::sched::offers().ok().and_then(|o| o.vector_bytes());
nexus_paint_kernels::choose(if width.is_some() { Unit::Vector } else { Unit::Scalar });
```

Once, before the first row (app-host: `probe/boot_env.rs`, `apphost: paint kernels (vector=
width=)`). The choice is the process's: every thread paints with it. Without a `choose`, on a
kernel that gives no answer and on every other architecture the scalar reference runs. The
kernel names the vector unit only when every CPU that runs threads has it at one width and the
kernel keeps a thread's registers (RFC-0119) — a program never tries an instruction to find
out.

A span shorter than `SHORT` (64 bytes, sixteen pixels) is painted by the scalar form whatever
was chosen. The first vector instruction of a thread makes the kernel keep 32 registers for it
from then on (1 KiB on the board); a thread that paints nothing longer never pays that.

## What proves a vector form

| Proof | Where | Says |
|---|---|---|
| `the_reference_is_the_painters_pixel_…`, `no_sum_passes_sixteen_bits_…` | host tests of the crate | the reference is the arithmetic the painter always had; the vector form's division is exact |
| `proof::run()` = `proof::DIGEST` | host test, the selftest, the measurement program | 8 192 spans no caller picks — any length to 1 100 bytes, any byte offset, whole pixels or not, every alpha — painted by the kernels as chosen and by the reference: no byte differs, in the span or around it, and the digest of what the kernels left is the number the reference yields on the host |
| `SELFTEST: paint kernels ok (vector= width= spans= digest=)` | every QEMU lane, the board | the same proof on our kernel, with the unit chosen from the kernel's answer the way app-host chooses |
| `rasterbench-hard kernels` | the user-mode emulator at 16, 32, 64 and 128-byte registers, the board's stock system | the same proof at widths no machine of ours has, and the painter's seven scene digests unchanged |
| the host goldens | `just test-host` | the painter through the reference paints what it painted |

`scripts/qemu-test.sh` fails a run in which the kernel offered the vector unit and a program's
`paint kernels` line says `vector=0`: the mechanism unused is a failure, not a fallback.

## Adding a kernel

1. Write the scalar form in `reference.rs` and move the caller's loop onto it — the goldens
   prove nothing changed.
2. Add its spans to `proof::run()` and take the new `DIGEST` from the host test.
3. Write the vector form in `riscv.rs`: the step from `vsetvli`, at most one group asked for,
   bytes across memory, no width named.
4. Run `rasterbench-hard kernels` under the user-mode emulator at several widths
   (`qemu-riscv64 -cpu max,vlen=128|256|512|1024`), then the QEMU lanes: the digest is the
   host's or the form is wrong.
5. Measure it on the board's stock system before it is called a gain
   ([the measurement](../board/measurements/2026-11-13-paint-kernels/README.md)).
