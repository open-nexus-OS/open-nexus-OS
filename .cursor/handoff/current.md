# Handoff — RFC-0059 Display Pipeline: Phase A–D COMPLETE

Date: 2026-05-31

## Status

Four phases implemented. Pending cross-compile verification and QEMU smoke test.

### Done

- ✅ **Phase A**: gpud 1280×800 display resource + virtio-gpu primary display
- ✅ **Phase B**: fbdevd Priority-0 (3rd service) + bulk splash write
- ✅ **Phase C**: GPU zero-copy path — windowd→gpud VMO handoff
- ✅ **Phase D**: windowd defensive init + wallpaper fallback + retry backoff
- ✅ RFC-0059, CHANGELOG, .cursor/current_state.md updated

### Files changed

| File | Change |
|------|--------|
| `source/drivers/gpud/src/service.rs` | 1280×800 constants, new OP_SET_FRAMEBUFFER_VMO, MovedCap receive |
| `source/drivers/gpud/src/backend.rs` | `attach_external_framebuffer()` — zero-copy VMO→virtio-gpu scanout |
| `source/drivers/gpud/src/markers.rs` | GPUD_SCANOUT_MODE, GPUD_DISPLAY_READY |
| `scripts/run-qemu-rv64.sh` | virtio-gpu before ramfb, QEMU_GPU_DEVICE_PLACED guard |
| `source/apps/init-lite/build.rs` | fbdevd moved to position 3 (Priority-0) |
| `source/services/fbdevd/src/splash.rs` | Bulk VMO write (1 call vs 800) |
| `source/services/fbdevd/src/os_lite.rs` | Exponential backoff + retry counter for windowd registration |
| `source/services/windowd/src/markers.rs` | RUNTIME_INIT_START/OK, WALLPAPER_LOADED/FALLBACK/FAIL |
| `source/services/windowd/src/compositor/mod.rs` | Defensive runtime init with error diagnostics |
| `source/services/windowd/src/compositor/runtime.rs` | Wallpaper fallback, try_handoff_framebuffer_to_gpud(), GPU_SET_FRAMEBUFFER_VMO_OP |

### Next step

```bash
# Cross-compile verification
just build

# Interactive smoke test
just start

# Proof-mode marker ladder
RUN_UNTIL_MARKER=1 NEXUS_DISPLAY_BOOTSTRAP=1 just test-os visible-bootstrap
```

Expected new markers in order:
```
gpud: virtio-gpu probed
gpud: scanout ok
gpud: scanout 1280x800 bgra8888
gpud: display ready (w=1280, h=800)
windowd: runtime init start
windowd: wallpaper loaded (jpeg)          # or: wallpaper fallback solid
windowd: runtime init ok
windowd: ready (w=1280, h=800, hz=120)
windowd: fb handoff to gpud ok           # if gpud reachable
```

### Debug if stuck

- If `gpud: mmio fault` on create_resource(1280,800): increase GPU_RESOURCE_STRIDE or check VMO size limit
- If QEMU window not 1280×800: verify virtio-gpu is before ramfb in QEMU args; check `-display gtk` version
- If splash still slow: check fbdevd service position in UART log (`init: start fbdevd` should appear early)
- If `windowd: wallpaper fail`: verify `resources/wallpapers/base/default.jpeg` exists and is valid JPEG
