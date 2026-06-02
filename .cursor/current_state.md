# Current State — Open Nexus OS

Last updated: 2026-06-02

## Active focus

**TASK-0062 Phase 6: GPU-only display architecture — windowd sole owner**

## Architecture decision

Removed fbdevd/ramfb from the OS graph entirely. The display architecture
now follows the OHOS/Fuchsia/Android pattern:

```
windowd (sole display owner)
  │
  ├── vmo_create(1280×800×4)
  ├── compose frames
  ├── OP_SET_FRAMEBUFFER_VMO → gpud
  └── OP_UPDATE_VISIBLE_STATE ← inputd
        │
        ▼
┌──────────────────┐
│  gpud (driver)    │
│  probe virtio-gpu │
│  ATTACH_BACKING   │
│  SET_SCANOUT      │
└──────────────────┘
```

**One owner, one path. No fbdevd, no ramfb, no handoff from another service.**

## What changed

### Removed from active graph
- **fbdevd**: No longer spawned by init-lite (removed from `build.rs` default_candidates)
- **ramfb**: QEMU device already absent in `scripts/run-qemu-rv64.sh`

### gpud — pure driver (no display ownership)
- `service_main_loop`: only `probe` + `gpud: ready`. No `create_resource`/`set_scanout`/splash at startup
- `OP_SET_FRAMEBUFFER_VMO` handler: `attach_external_framebuffer` (create + attach_backing + set_scanout)
- Splash module removed (`splash.rs` deleted)
- Unused imports cleaned (`GfxBackend`, `PixelFormat`)

### windowd — always self-bootstrap
- `compositor/mod.rs`: Always creates own framebuffer VMO (`vmo_create`), no fbdevd check
- Removed `OP_SEND_COMPOSED_FRAME_VMO` handler (fbdevd VMO handoff path)
- Removed `KernelClient` import (no longer used in compositor main loop)
- Zero-copy handoff to gpud via `try_handoff_framebuffer_to_gpud` (runtime.rs)

### init-lite
- `build.rs`: `"fbdevd"` removed from `default_candidates`

### selftest observer
- `display_bootstrap_observer.rs`: `route_with_retry("fbdevd")` → `route_with_retry("windowd")`
- `tests/boot_cfg_runtime.rs`: test updated to expect windowd

### Markers
- `qemu-test.sh`: fbdevd markers removed, GPU markers updated
- `proof-manifest/markers/bringup.toml`: fbdevd entries removed
- `proof-manifest/markers/ui.toml`: architecture comment updated, marker name changed

### Spec-validation tests (16 new)
- `tests/protocol_tests.rs`: Format constants, command types, response types, MMIO offsets, struct sizes

## Test status

| Suite | Result |
|-------|--------|
| gpud (20 tests) | ✅ 20/20 |
| windowd (44 tests) | ✅ 44/44 |
| selftest-client (10 tests) | ✅ 10/10 |
| Total | ✅ 74/74 |

## Pending verification

- ⬜ `just test-os visible-bootstrap` — needs display (GTK unavailable in headless env)
- ⬜ `just start` — interactive manual verification
