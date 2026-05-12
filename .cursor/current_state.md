# Current State — Open Nexus OS

Last updated: 2026-05-12 (TASK-0056C re-opened)

## What changed

TASK-0056C (UI v2a present/input perf latency coalescing) was marked Done but
re-opened to In Progress. Root cause: the 56C fastpath/coalescing/skip code
exists in `windowd/src/server.rs` but is dead in the live OS path — `enable_fastpath()`,
`try_coalesce_pointer_move()`, `try_no_damage_skip()`, and `present_tick()` are
never called from any OS service event loop. The 22 host tests pass because they
call the API directly, but the interactive QEMU path (`just start`) is unchanged.

RFC-0055 also back to In Progress.

## Pipeline bottlenecks found

1. `hidrawd -> inputd`: per-batch blocking IPC (send+recv per mouse event)
2. `inputd`: single-threaded — HID batches and fbdevd queries in same loop
3. `fbdevd -> inputd`: separate IPC per frame (2ms timeout)
4. No hardware vsync — software timer at 60Hz
5. `windowd` is not a daemon — WindowServer embedded in inputd, no own loop

## Plan

1. Wire fastpath + present_tick into inputd OS loop
2. Add windowd os_lite.rs daemon with own compose/present cadence
3. fbdevd -> windowd direct present path
4. IPC optimization: client caching per RFC-0026
5. Per-hop tests per hardening matrix
6. Test with `just test-os visible-bootstrap` + `just start`

## Known risks

- DON'T bypass inputd/windowd authority boundaries for speed
- DON'T coalesce click/focus/wheel/keyboard edges
- DON'T claim Done until interactive mouse latency is visibly improved
- DON'T add prints/logs/markers in kernel
