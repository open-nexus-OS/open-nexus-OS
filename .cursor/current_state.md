# Current State — Open Nexus OS

Last updated: 2026-06-01

## Active focus

**Infrastructure fix: nexus-init OS build regression + CI hygiene**

### Fixed ✅ — nexus-init RFC-0061 incomplete refactoring
- `lib.rs`: added missing `extern crate alloc;` (required for `no_std` builds)
- `Cargo.toml`: added `[[bin]] required-features = ["std-server"]` (binary was incorrectly compiled for RISC-V)
- `os_payload.rs`: added `pub(crate) use` re-exports for items moved to `bootstrap/` during RFC-0061
- `os_payload.rs`: made `Result<T>`, `CTRL_*`, `MAX_LOG_STR_LEN`, `PROBE_ENABLED`, `log_topics`, extern symbols `pub(crate)`
- `bootstrap/helpers.rs`: added `pub(crate)` visibility to functions/constants used by sibling modules
- `bootstrap/helpers.rs`: added missing imports (`LineBuilder`, `log_topics`, `Result`, extern symbols)

### Fixed ✅ — gpud compiler warnings
- `backend.rs`: `|e|` → `|_e|` in 3 closures
- `backend.rs`: `#[allow(dead_code)]` on `ResourceRecord` and `CURSOR_QUEUE_INDEX`

### Fixed ✅ — windowd compiler warnings
- `backdrop.rs`: removed unused imports (`LayerCache`, `PathCacheEntry`, `BACKDROP_CACHE_ENTRIES`, etc.)

### Fixed ✅ — proof-manifest marker
- `markers/ui.toml`: `fbdevd: ready` changed from `phase=end emit_when={profile=visible-bootstrap}` to `phase=bringup` (fbdevd now starts early per RFC-0059 Phase B)

## CI Status

| Gate | Status | Notes |
|------|--------|-------|
| `just ci-network` (dhcp) | ✅ PASS | qemu_status=0, all selftests pass |
| `just ci-network` (quic-required) | ✅ PASS | qemu_status=0 |
| `just ci-network` (os2vm) | ✅ PASS | qemu_status=0, verify-uart clean |
| `just test-all` (fmt+lint+deny+host+e2e+miri+arch+kernel) | ✅ PASS | all gates pass |
| `just test-all` (ci-os-smp SMP=2) | ⚠️ TIMEOUT | Pre-existing: SMP=2 hangs in FPS idle loop after dsoftbusd:ready |

## Pending verification

- ⬜ SMP=2 timeout root cause (likely display pipeline RFC-0059 race condition)
- ⬜ windowd remaining warnings (53 warnings in os-lite cross-compile, non-blocking)
- ⬜ `just dep-gate` verification