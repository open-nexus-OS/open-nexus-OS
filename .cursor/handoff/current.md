# Handoff — TASK-0062 Phase 6 GPU-Only Architecture

Date: 2026-06-02

## Status

Host tests: ✅ 74/74 (gpud 20 + windowd 44 + selftest-client 10)
QEMU proof: ⬜ pending (needs GTK display; guest emits all markers up to `SELFTEST: ui v3 effect ok`)

## Architecture: windowd sole display owner

```
windowd → vmo_create → gpud (ATTACH_BACKING + SET_SCANOUT)
   ↑                         │
   │ OP_UPDATE_VISIBLE_STATE  │ virtio-gpu
   │                         ▼
 inputd ← hidrawd/touchd    QEMU GTK (dev only)
```

fbdevd and ramfb are removed from the OS graph entirely.

## What was done

- gpud: startup scanout/splash removed; pure driver now
- windowd: always self-bootstraps; removed OP_SEND_COMPOSED_FRAME_VMO handler
- init-lite: fbdevd removed from default_candidates
- selftest observer: routes to windowd instead of fbdevd
- Markers: qemu-test.sh, bringup.toml, ui.toml all updated
- Tests: 16 spec-validation tests for virtio-gpu protocol constants
- Cleanup: splash.rs deleted

## Key files changed

| File | Change |
|------|--------|
| `source/drivers/gpud/src/service.rs` | Startup: remove create_resource/set_scanout/splash; only probe + ready |
| `source/drivers/gpud/src/lib.rs` | Remove splash module |
| `source/drivers/gpud/src/splash.rs` | DELETED |
| `source/drivers/gpud/tests/protocol_tests.rs` | 16 new spec-validation tests |
| `source/services/windowd/src/compositor/mod.rs` | Always self-bootstrap; remove OP_SEND_COMPOSED_FRAME_VMO |
| `source/apps/init-lite/build.rs` | Remove fbdevd from default_candidates |
| `source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs` | route_with_retry("windowd") |
| `source/apps/selftest-client/tests/boot_cfg_runtime.rs` | Test expects windowd |
| `scripts/qemu-test.sh` | Remove fbdevd/ramfb markers; add GPU markers |
| `source/apps/selftest-client/proof-manifest/markers/bringup.toml` | Remove fbdevd entries |
| `source/apps/selftest-client/proof-manifest/markers/ui.toml` | Update arch comment + marker names |

## Next step

```bash
# Primary verification (needs display)
just test-os visible-bootstrap

# Interactive verification
just start
```

Expected marker sequence:
```
gpud: virtio-gpu probed → gpud: ready
windowd: backend=gpu → windowd: compose ready
windowd: backend=visible → windowd: present visible ok
SELFTEST: ui visible present ok → SELFTEST: ui v3 effect ok
```
