# ADR-0072: GPU programs are written in WGSL, lowered into our own IR and compiled by our backends into a pipeline library per GPU — the driver holds none

- Status: Accepted
- Date: 2026-10-15
- Links:
  - Tasks: `tasks/TASK-0215-desktop-on-the-gpu-render-queue-pipelines-cpu-composite-deleted.md` (5c-2: execution + proof)
  - RFCs: `docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md`
    (§7 GPU code is client data; §7b the pipeline library — the contract this decision feeds),
    `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md` (its open question
    "SPIR-V or custom IR?" — answered here), `docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md`
    (the virtual GPU's hand-written shaders)
  - Measurements: `docs/board/measurements/2026-10-13-gpu-render-queue/README.md` (5c-1),
    `docs/board/measurements/2026-10-10-gpu-stock-baseline/README.md` (the bar)

## Context

- **A pixel costs its program.** On the board's GPU a pixel costs 0.15 ns plus 0.206 ns per
  instruction group of its fragment program (5c-1: four programs on one line, 55 to 116 groups).
  The open userspace driver's compiler spends 55–68 groups on a textured quad. Most of them are
  generality an application API needs and a compositor does not: a helper-lane feedback that
  makes every program punch-through, a dynamic alpha-to-coverage path, a read-back of the tile
  for its sample check (so no program of that compiler is ever opaque to the tile hardware),
  moves that zero unused sampler coordinates. The stock stack's opaque fill sits at about eight
  groups on the same line, its further blended layer at about four.
- **The GPU family has no fixed-function blending.** Blending, output formats and vertex fetch
  are program code (the program reads the tile, mixes, writes; setup programs fetch attributes).
  Whoever produces the programs produces these parts too.
- **The structure is already chosen around it.** RFC-0101 §7: GPU code is client data; the
  driver holds no built-in shader; capabilities name the device's code target. The GPU base
  structure agreed on 2026-10-10: offline now, a JIT later as a library above the driver.
  `nexus_gfx::shader::{module, library}` and `nexus_gfx::pipeline` exist as placeholders (a
  pre-compiled, signed archive; render pipeline descriptors). RFC-0059 left the binary format
  open ("SPIR-V or custom IR?").
- **Today every compositor op exists twice.** The CPU rasterizer of nexus-gfx is the reference;
  the virtual GPU of the QEMU path runs hand-written shader text for the same ops; the board's
  GPU runs programs captured from the open compiler for measurement only.
- **The longer horizon** (operator, 2026-10-15): more GPUs, one day our own, and a platform at
  the level of the reference desktop for games, professional creative work and machine
  learning. The program architecture must carry that without a transition.

## Decision

1. **WGSL is the source language** of every GPU program in the system — the compositor's first,
   application code later.
2. **The frontend is naga 28.x (the WGSL parser and validator of an open graphics project, MIT
   or Apache-2.0), on the host only**, in a build tool; it never enters the OS graph. Its IR is
   lowered at once into ours. (28 is the newest release the pinned toolchain builds: 29 needs a
   newer compiler.)
3. **Our IR is the center** (`nexus_gfx::shader::ir`): SSA with structured control flow; typed
   scalars and vectors including 16-bit floats; the interface (inputs, outputs, resources,
   constants) declared, not inferred; intrinsics for sampling, fused arithmetic, packing. It is
   `no_std`, so the same code can later be the JIT library. Later frontends (SPIR-V, DXIL, a
   machine-learning graph compiler) lower into it. **No frontend's IR is ever the system's.**
4. **Backends are ours, one per code target** (RFC-0101 `CodeTarget`):
   - `rogue` — the board's GPU: shader-core code, the setup programs that kick it, and the state
     words a pipeline fixes (pass type, output format);
   - `tgsi` — the virtual GPU of the QEMU path; it replaces the hand-written shader text;
   - an interpreter of our IR on the host — the reference that the CPU rasterizer's formulas
     judge, so every program is proven before it meets a GPU.
   A target that already has a mature open compiler may lower our IR into that compiler's input
   instead of a backend of ours (the valve for foreign GPUs). Our own backends are written where
   nothing good exists — this GPU today — and for our own hardware.
5. **A program is linked from parts**: a main part, an epilog (blend, output format, sample
   mask) and, for vertex work, a prolog (attribute fetch). At build time every variant the
   library declares is linked whole; the parts are the fast path of the JIT later. A main
   part's variants are WGSL overrides, fixed per pipeline when it is built (keyed as a
   pipeline's constants are: an override's `@id`, else its name); a `select` on one folds in the
   frontend, so a variant carries no instruction of the paths it switched off — the opaque layer
   at full opacity has no tint multiply (TASK-0215 5c-2 P3c).
6. **The pipeline library is one archive per code target**, built at OS build time. An entry
   carries its id, code target, interface, program bytes, setup programs, state words, its
   budget and a digest. The format lives in `nexus_gfx::shader::library`, the contract in
   RFC-0101 §7b. The client (windowd) supplies the library; gpud validates and binds it and
   holds no program of its own.
7. **A budget is part of a pipeline's definition**: its instruction-group count. A host test
   fails above it; the board measures ns a pixel on the 5c-1 line against the stock's numbers.
8. **The proof method**: the `rogue` encoder disassembles and re-encodes the captured programs
   byte for byte; every encoding has a golden from the open compiler's encoder; the host
   interpreter and the CPU rasterizer give the pixel goldens; the board measures.
9. **The formats keep the horizon open**: the IR and the library are versioned and extensible
   toward bindless resources, indirect work, subgroup and matrix operations, mesh and ray
   workloads. The compositor needs none of it now; nothing in the formats may exclude it.

Out of scope for this decision: the SPIR-V and DXIL frontends, the JIT, other GPUs' backends,
an application-facing shader API.

## Consequences

- **Positive**: compositor programs at the hardware's own budget (opaque layers opaque to the
  tile hardware, blending a few groups); one source for the board and the QEMU path, so the
  hand-written shader text retires; a compiler stack of our own — the precondition for our own
  hardware and for the JIT; RFC-0059's question closed.
- **Negative / accepted cost**: a compiler backend is the largest piece of the GPU lane; it is
  cut to the compositor's subset first and grows by measured need. naga becomes a host build
  dependency (`tools/nxgpu`); its WGSL parser brings `hexf-parse` under CC0-1.0, admitted by a
  per-crate exception in `config/deny.toml` (a public-domain dedication; nothing of it reaches the
  OS graph). The IR needs care: it is versioned from the first entry.
- **Follow-ups**: TASK-0215 5c-2 (the IR, the `rogue` backend with its goldens, the first
  pipelines on the board, the library and its loading path, the `tgsi` backend and the QEMU path
  on the library); RFC-0101 §7b; the memory lane's GPU-only and destroyable buffers before the
  frame path; frontends, JIT and the application API as their own tasks.

## Alternatives considered

- **The open compiler's programs, captured** (rejected): measured at 55–68 groups against about
  eight; the generality of an application API is baked into every program; an experimental
  compiler outside our tree.
- **The open compiler, patched** (rejected): C outside our tree, it cannot become the in-system
  JIT library, and its IR carries the same generality.
- **SPIR-V as the system IR** (rejected as the center): large and shaped by one API's semantics;
  accepted as a later frontend.
- **naga's IR as the system IR** (rejected): owned by another project and bound to the web's
  feature level; how fast it gains what games and machine learning need is not ours to decide.
- **A shading language of our own** (rejected): it would reinvent what a standard with a
  validator already gives.
- **Hand-written assembly per program** (rejected): does not scale to more ops or more GPUs; the
  encoder exists anyway, and programs come from the compiler.
