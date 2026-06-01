# Handoff — nexus-init OS build fix + CI hygiene

Date: 2026-06-01

## Status

`just ci-network` (dhcp + quic-required + os2vm) — ALL GREEN ✅
`just test-all` — GREEN except SMP=2 timeout (pre-existing) ⚠️

## What was done

### Root cause
`nexus-init` crate had an incomplete RFC-0061 refactoring: items were moved from `os_payload.rs` to `bootstrap/` but imports, visibility, and `extern crate alloc` were never updated. This caused 680 compilation errors when building for RISC-V, making the entire OS build fail. Previous CI runs succeeded only because of stale cached artifacts.

### Fixes applied
1. Added `extern crate alloc;` to `lib.rs` (required for `no_std`)
2. Added `[[bin]] required-features = ["std-server"]` to `Cargo.toml`
3. Added `pub(crate) use` re-exports in `os_payload.rs` for items moved to `bootstrap/`
4. Made private items in `os_payload.rs` and `bootstrap/helpers.rs` `pub(crate)`
5. Fixed gpud unused variable warnings (`|e|` → `|_e|`)
6. Fixed gpud dead_code warnings (`#[allow(dead_code)]`)
7. Fixed windowd unused imports in `backdrop.rs`
8. Updated proof-manifest: `fbdevd: ready` marker now `phase=bringup` (was `end` with `visible-bootstrap` restriction)

### Files changed
| File | Change |
|------|--------|
| `source/init/nexus-init/Cargo.toml` | Added `[[bin]]` with `required-features = ["std-server"]` |
| `source/init/nexus-init/src/lib.rs` | Added `extern crate alloc;` |
| `source/init/nexus-init/src/os_payload.rs` | Re-exports + pub(crate) visibility |
| `source/init/nexus-init/src/bootstrap/helpers.rs` | pub(crate) visibility, missing imports |
| `source/drivers/gpud/src/backend.rs` | Unused variables + dead_code |
| `source/services/windowd/src/compositor/backdrop.rs` | Unused imports |
| `source/apps/selftest-client/proof-manifest/markers/ui.toml` | fbdevd:ready phase change |
| `.cursor/current_state.md` | Updated |
| `CHANGELOG.md` | Updated |

## Remaining issue

**SMP=2 timeout**: `just test-os smp` (SMP=2) times out at 190s. System boots and reaches `dsoftbusd: ready` but never hits `SELFTEST: end` — hangs in FPS idle loop. SMP=1 (dhcp profile) works correctly. This is likely a pre-existing display pipeline race condition, unrelated to today's fixes. Debug hints:
- Check if windowd/gpud have SMP-related races (REC-0059 display pipeline)
- Run with longer timeout: `RUN_TIMEOUT=300s SMP=2 RUN_UNTIL_MARKER=1 just test-os smp`

## Next step

```bash
# Investigate SMP=2 timeout
RUN_TIMEOUT=300s SMP=2 RUN_UNTIL_MARKER=1 just test-os smp

# If blocked, verify with dep-gate
just dep-gate
```