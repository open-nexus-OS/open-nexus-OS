<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The board GPU's firmware (TASK-0329 G1, RFC-0101 Phase 2)

The published, redistributable firmware for the board's GPU core, unmodified, with its licence.
gpud's GPU backend loads it by the image layout the open kernel driver documents; the blob is
never disassembled (the licence forbids it) and never patched (`scripts/check-firmware-provenance.sh`
holds every blob under `resources/firmware/` to the hash named here, in `just check`).

| | |
|---|---|
| File | `rogue_36.29.52.182_v1.fw` |
| For | the core with id `36.29.52.182` (the board tree's `gpu@cac00000`, `compatible = "img,rgx"`; G0 read the id from the core's register: `0x0024001D003400B6`) |
| Size | 126 976 bytes (31 blocks of 4 KiB) |
| sha256 | `1ca06dd4c721751b45ab023ce35790d5245c4fed0d636dd03b973d1fea478e30` |
| Version | 1.1, build 6976702 (`WHENCE`: `Version: 1.1.OS@6976702`); the trailer's flag bit 0 = the open-source build |
| Source | the GPU vendor's firmware tree on freedesktop, branch `powervr`, commit `8a58f81883f7be458daa34e418cc4079f995b279` (2026-04-15), path `powervr/rogue_36.29.52.182_v1.fw`; the same `WHENCE` block covers sixteen cores with the same version line |
| Licence | `LICENSE` beside it (the tree's `LICENSE.powervr`): redistribution and use in binary form without modification; the copyright notice and disclaimer reproduced in the distribution's documentation — this file; no endorsement; no reverse engineering, decompilation or disassembly; only with products designed and licensed by the vendor (the board's SoC is one); a limited patent licence for the software alone or combined with an OSI-approved operating system (this project is Apache-2.0) |
| Not this | the stock image's `rgx.fw.36.29.52.182` (139 264 bytes, release 24.2, build 6603887): another build with a closed host interface; its licence is unknown and it is not in this tree |

## Image layout (from the trailer, as the loader reads it)

`info_version 3`, header 48 bytes, six layout entries, `fw_page_size 4096`, flags `0x1`:

| Section | Kind | Firmware virtual base | Max | Alloc size | Alloc offset |
|---|---|---|---|---|---|
| MIPS code | code | `0xC0000000` | `0x2F000` | `0x17000` | `0x0` |
| MIPS exceptions | code | `0x9FC02000` | `0x2000` | `0x1000` | `0x17000` |
| MIPS boot code | code | `0xBFC00000` | `0x1000` | `0x1000` | `0x18000` |
| MIPS private data | data | `0xC0032000` | `0x8000` | `0x3000` | `0x0` |
| MIPS boot data | data | `0xBFC01000` | `0x1000` | `0x1000` | `0x3000` |
| MIPS stack | data | `0xCF600000` | `0x1000` | `0x1000` | `0x4000` |

The host model (`nexus_gfx::backend::gpu::fw_image`) parses this blob in its tests and refuses a
blob whose id, flag or version differs (`test_reject_firmware_*`).
