// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Marker strings for the gpud service.
pub const GPUD_READY: &str = "gpud: ready";
pub const GPUD_VIRTIO_GPU_PROBED: &str = "gpud: virtio-gpu probed";
pub const GPUD_NO_DEVICE: &str = "gpud: no device";
pub const GPUD_CURSOR_ON: &str = "gpud: cursor on";
pub const GPUD_SCANOUT_OK: &str = "gpud: scanout ok";
// Size-agnostic (the dynamic `gpud: display info WxH` line carries the mode).
pub const GPUD_SCANOUT_MODE: &str = "gpud: scanout bgra8888";
pub const GPUD_DISPLAY_READY: &str = "gpud: display ready";
pub const GPUD_MMIO_FAULT: &str = "gpud: mmio fault";
pub const GPUD_RESOURCE_VMO_CREATE_FAIL: &str = "gpud: resource vmo_create fail";
pub const GPUD_RESOURCE_VMO_MAP_FAIL: &str = "gpud: resource vmo_map_page fail";
pub const GPUD_RESOURCE_CAP_QUERY_FAIL: &str = "gpud: resource cap_query fail";
pub const GPUD_CB_RENDER_OK: &str = "gpud: cb render ok";
pub const GPUD_RESOURCE_CREATE_CMD_FAIL: &str = "gpud: resource create cmd fail";
pub const GPUD_RESOURCE_ATTACH_CMD_FAIL: &str = "gpud: resource attach cmd fail";
pub const GPUD_RESOURCE_CREATED: &str = "gpud: resource created";
pub const GPUD_IPC_READY: &str = "gpud: ipc ready";
pub const GPUD_ANIMATION_SUBMIT_OK: &str = "gpud: animation submit ok";
pub const GPUD_ANIMATION_SUBMIT_FAIL: &str = "gpud: animation submit fail";
pub const GPUD_REQUEST_MALFORMED: &str = "gpud: request malformed";
/// Emitted when virgl GPU acceleration is detected and active.
pub const GPUD_VIRGL_READY: &str = "gpud: virgl ready";
/// First destination-so-far glass backdrop (RT snapshot + GPU blur) submitted —
/// glass layers now blur what is actually beneath them (lower windows, chrome)
/// instead of the static wallpaper texture (TASK-0070 Phase 4).
pub const GPUD_RT_BACKDROP_DST: &str = "gpud: rt backdrop dst ok";
/// Emitted when virgl is not detected — CPU fallback is active.
pub const GPUD_CPU_FALLBACK: &str = "gpud: cpu fallback";
/// Emitted when a minimal SUBMIT_3D command stream is accepted by virglrenderer
/// (validates the 3D wire format + context routing before the blur shader lands).
pub const GPUD_VIRGL_SUBMIT3D_OK: &str = "gpud: virgl submit3d ok";
/// Emitted when a 3D render-target resource is created, bound as a framebuffer
/// surface, and cleared — validates the draw-state path (resource → surface →
/// framebuffer → clear) before shaders/draw are added.
pub const GPUD_VIRGL_RT_CLEAR_OK: &str = "gpud: virgl rt clear ok";
/// Emitted after submitting vertex+fragment TGSI shaders for creation —
/// validates the CREATE_OBJECT(SHADER) text path is parsed by virglrenderer
/// (confirm zero virgl errors in QEMU stderr).
pub const GPUD_VIRGL_SHADER_OK: &str = "gpud: virgl shader ok";
/// Emitted when a full GPU draw (state objects + vertex buffer + shaders +
/// DRAW_VBO) renders the expected pixels, verified by TRANSFER_FROM_HOST_3D
/// readback into guest memory. This is an on-device end-to-end GPU proof.
pub const GPUD_VIRGL_DRAW_OK: &str = "gpud: virgl draw ok";
/// Draw self-test readback returned the clear color — pipeline state reached
/// the GPU but the draw itself produced no fragments.
pub const GPUD_VIRGL_DRAW_NOOP: &str = "gpud: virgl draw noop (clear only)";
/// Draw self-test readback returned unexpected pixels.
pub const GPUD_VIRGL_DRAW_MISMATCH: &str = "gpud: virgl draw mismatch";
/// Emitted on the first real GPU blur when its output matches the CPU
/// separable gaussian within tolerance over the region interior.
pub const GPUD_VIRGL_BLUR_PARITY_OK: &str = "gpud: virgl blur parity ok";
/// First-blur parity comparison exceeded tolerance (GPU blur still active;
/// indicates a kernel/orientation deviation to investigate).
pub const GPUD_VIRGL_BLUR_PARITY_OFF: &str = "gpud: virgl blur parity off";
/// Emitted once when the first BlurBackdrop is executed by the GPU shader path.
pub const GPUD_VIRGL_BLUR_GPU_ON: &str = "gpud: virgl blur gpu on";
/// GPU vector pipeline (M1): a per-pixel gradient quad rendered + read back with
/// top≠bottom interpolation — proves GPU gradient fills work end-to-end.
pub const GPUD_VIRGL_GRADIENT_OK: &str = "gpud: virgl gradient ok";
/// Gradient self-test reached the GPU but readback showed no interpolation.
pub const GPUD_VIRGL_GRADIENT_FLAT: &str = "gpud: virgl gradient flat";
/// G0: the displayed scanout is a virgl render target (GL-presented). Emitted
/// once when SET_SCANOUT to the GL RT + GPU clear + flush succeeded.
pub const GPUD_GL_SCANOUT_OK: &str = "gpud: gl scanout ok";
/// G1: first VMO→GL present executed (upload + GPU blit + flush).
pub const GPUD_GL_PRESENT_OK: &str = "gpud: gl present ok";
/// GL scanout init failed; display fell back to the 2D transfer/flush path.
pub const GPUD_GL_SCANOUT_FALLBACK: &str = "gpud: gl scanout fallback 2d";
/// G3/M1b: first FillSdfGradient executed by the GPU SDF shader.
pub const GPUD_SDF_GRAD_OK: &str = "gpud: sdf-grad ok";
/// G3/M1c: first DropShadow executed by the GPU SDF-falloff shader.
pub const GPUD_DROPSHADOW_OK: &str = "gpud: dropshadow ok";
/// G2: the GPU layer-composite primitive (textured layer + rounded mask +
/// opacity composited into an RT) verified by readback at bringup.
pub const GPUD_LAYER_COMPOSITE_OK: &str = "gpud: layer composite ok";
/// Layer-composite self-test reached the GPU but readback was wrong.
pub const GPUD_LAYER_COMPOSITE_OFF: &str = "gpud: layer composite off";
/// G2 live: first window/layer GPU-composited into the display plane from a
/// CompositeLayer command (windowd → gpud).
pub const GPUD_LAYER_COMPOSITE_LIVE: &str = "gpud: layer composite live";

// --- Present-chain hop markers (graphical-output bisection) -----------------
// One frame's journey through gpud's present chain, emitted in order so a
// headless run shows exactly HOW FAR a frame gets: the last hop printed is the
// last stage reached. Traced one-shot once a frame completes cleanly, but
// re-armed every frame while the chain is broken. Failures carry a reason.
// Kept string-identical to the GpudContract spec in tools/nx so the chain test
// verifies the same hop order the real service emits.
/// G1: gpud received an OP_PRESENT_DAMAGE frame from windowd.
pub const GPUD_CHAIN_RECV: &str = "gpud: chain G1 recv present-damage";
/// G2: the serialized command buffer deserialized (reload_from) cleanly.
pub const GPUD_CHAIN_PARSE_OK: &str = "gpud: chain G2 parse ok";
/// G2 fail: the command buffer was rejected (truncated / bad tag / over cap).
pub const GPUD_CHAIN_PARSE_FAIL: &str = "gpud: chain G2 parse FAIL (command buffer rejected)";
/// G3: present_committed executed every blit/composite command.
pub const GPUD_CHAIN_EXEC_OK: &str = "gpud: chain G3 exec ok (commands applied)";
/// G3 fail: a command failed during present_committed (reason follows).
pub const GPUD_CHAIN_EXEC_FAIL: &str = "gpud: chain G3 exec FAIL";
/// G4: the frame was transferred/blitted to the scanout and flushed.
pub const GPUD_CHAIN_SCANOUT_OK: &str = "gpud: chain G4 scanout ok (frame presented)";
/// G4 fail: the scanout present failed (sub-stage reason follows on virgl).
pub const GPUD_CHAIN_SCANOUT_FAIL: &str = "gpud: chain G4 scanout FAIL";
/// G3b: the whole present (all SUBMIT_3D draws + the flush) was ENQUEUED into the
/// multi-entry command ring without per-command waits — the batched-submit path.
/// A real run printing this confirms the present reached the ring as one batch.
pub const GPUD_CHAIN_BATCH_SUBMIT: &str = "gpud: chain G3b batch submit ok (present enqueued)";
/// G3c: the batch drained — every enqueued command completed via ONE reactive
/// GPU-ring-buffer-IRQ wait. This is the fix for the texture-sampling stall: a
/// deferred-completion draw no longer blocks the next command, only the drain.
pub const GPUD_CHAIN_BATCH_OK: &str = "gpud: chain G3c batch complete (drained)";
/// G3c fail: the batch drain timed out (a command's used-ring never advanced
/// within the deadline) — degraded, best-effort; the frame is abandoned.
pub const GPUD_CHAIN_BATCH_FAIL: &str = "gpud: chain G3c batch FAIL (drain timeout)";
