# Current State — Open Nexus OS

Last updated: 2026-05-31

## Active focus

**RFC-0059: Production-Grade Display Pipeline (Phase 3–6)**

### Phase A ✅ — gpud 1280×800 + virtio-gpu primary display
- gpud `PROOF_RESOURCE_W/H` → `DISPLAY_WIDTH`/`DISPLAY_HEIGHT` (1280×800)
- New markers: `gpud: scanout 1280x800 bgra8888`, `gpud: display ready (w=1280, h=800)`
- `run-qemu-rv64.sh`: virtio-gpu-device placed before ramfb (primary display)
- Duplicate QEMU_GPU_DEVICE guarded via QEMU_GPU_DEVICE_PLACED flag

### Phase B ✅ — Instant boot splash
- fbdevd moved to Priority-0 (3rd service, after keystored+identityd)
- splash.rs: single bulk `vmo_write` instead of 800 per-row calls
- Expected: splash visible <500ms (was ~6s)

### Phase C ✅ — GPU zero-copy display path
- gpud: new `OP_SET_FRAMEBUFFER_VMO` (op=3) + `attach_external_framebuffer()`
- windowd: `try_handoff_framebuffer_to_gpud()` sends FB VMO to gpud on registration
- windowd: `GPU_SET_FRAMEBUFFER_VMO_OP` constant (mirrors gpud::OP_SET_FRAMEBUFFER_VMO)
- Graceful degradation: CPU ramfb path active when gpud unreachable

### Phase D ✅ — windowd defensive init + diagnostics
- Wallpaper fallback: solid dark-blue 160×100 when JPEG unavailable
- New markers: `RUNTIME_INIT_START`, `RUNTIME_INIT_OK`, `WALLPAPER_LOADED`, `WALLPAPER_FALLBACK`
- fbdevd: exponential backoff for windowd registration (10ms→500ms)
- Diagnostic marker on 3rd retry: `fbdevd: windowd register retry`

## Key findings

- The display chain now has TWO paths: CPU ramfb (boot splash, fallback) and GPU virtio-gpu (primary, zero-copy)
- QEMU window size fix: virtio-gpu as primary display → GTK resizes on `SET_SCANOUT` with 1280×800
- fbdevd's early start means splash appears much sooner; windowd's fallback means compositor always starts
- gpud's `attach_external_framebuffer` uses `cap_query` on received VMO — same pattern as fbdevd's framebuffer alloc

## Pending verification

- ⬜ `just build` cross-compile: gpud, fbdevd, windowd crates
- ⬜ `just start` interactive: QEMU GTK window at 1280×800 via virtio-gpu
- ⬜ `RUN_UNTIL_MARKER=1 just test-os visible-bootstrap`: new marker ladder
- ⬜ gpud `create_resource(1280, 800, ...)` — verify no MMIO fault with 4MB resource
