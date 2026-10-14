<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# GPU scene bundles — GPU code as client data, captured on the host

A `.scene` file (`NXSC`, version 1; format and parser: `nexus_gfx::backend::gpu::scene`) carries
what the open userspace driver for one core submitted to the kernel for a small scene: every
buffer it bound (GPU address, length, kind, the non-zero bytes as sparse runs) and every job's
command stream. The device binds the same bytes at the same addresses, points the target at a
plane of the ring, generates the texture by the named formula, and submits the same jobs
(RFC-0101 §7 — the driver holds no shader; §8c — the transfer job). The bundle is untrusted
input: the parser checks the core id, the heaps, every run, the one target.

## Provenance

| Bundle | Core | Captured | Source of the bytes | Digest (FNV-1a 64) |
|---|---|---|---|---|
| `first-quad.scene` | BVNC 36.29.52.182 | 2026-10-13 | the open userspace driver (its Vulkan implementation, MIT licence; tree `26.3.0-devel`, commit `768b18d90b`) run on the host over its kernel-stand-in shim with the capture hook of `docs/board/measurements/2026-10-13-gpu-first-quad/capture/pvr_dump_hook.h`, driven by `capture/first_quad.c` | `0x699ff414b2672125` |
| `render-quad.scene` (version 2: the render trailer — the dataset's arguments, the geometry's call stack, the two free lists, the context's static state; since 2026-10-14 every buffer carries the open driver's flags: GPU cache allowed, PM/firmware protect) | BVNC 36.29.52.182 | 2026-10-14 (repacked; first captured 2026-10-13) | the same driver and hook, driven by `docs/board/measurements/2026-10-13-gpu-render-queue/capture/render_quad.c` (one blended textured quad through geometry, partial-render and fragment jobs; capture `variants/c_full`, its `log.txt` naming each buffer's creation flags); 37 buffers including the driver's 8 MiB partial-render scratch as zero data; the repack differs from the first capture only in the flags and in host-pointer residue inside unused sub-allocations | `0x8a7ed6a6b4442413` |

The programs (PDS, USC) and state words inside the bundle are the open driver's generated
output for this core; the texture is not carried (formula `Texture::CheckerV1`, digest
`CHECKER_V1_FNV1A64`), the target is not carried (a plane of the ring). The capture recipe,
the decoded streams and the buffer table are in the measurement folder's README.

## Repacking

```text
cargo run -p nexus-gfx --example scene_pack -- <capture-dir> resources/gpu/scenes/first-quad.scene
```

A repack changes the digest: update it here and in `userspace/nexus-gfx/tests/gpu_scene_goldens.rs`
(`the_bundle_is_the_capture_of_2026_10_13`), and record the new capture in the measurement
folder. The packer refuses a capture in which a buffer was rewritten between two jobs (the
device binds one version of every buffer for the whole scene).
