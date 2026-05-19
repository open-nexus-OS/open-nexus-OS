# Current State — Open Nexus OS

Last updated: 2026-05-19 (TASK-0059 Phase 6 complete + chain tests + ADR-0030 fix)

## Active task

TASK-0059: UI v3b clip/scroll/effects + IME stub + filter-box. — **Done**
Status: Phases 0-6 implemented (172+ tests, chain contract tests, ADR renumbering).
RFC-0058: Phases 0-6 checked — **Complete**.
Depends on: TASK-0058 (DONE).

## Post-audit fixes

### ADR-0029 duplicate resolved
- Renamed `0029-layout-engine-deterministic-pretext.md` → `0030-layout-engine-deterministic-pretext.md`
- Updated 4 references: `layout-types/src/{lib,border,node}.rs`, `tasks/TASK-0059-*.md`

### Chain contract tests added
- `tests/ui_v4_host/src/chain_tests.rs` (2 tests):
  - `test_inputd_to_windowd_hop_visible_state_contract`: wire format roundtrip → windowd state → composed frame
  - `test_windowd_to_fbdevd_hop_present_ack_contract`: compose → PresentAck → materialize frame → marker postflight reject
- Dependencies added: `input-live-protocol`, `windowd`

## Phase 6 summary

| Phase | Component | Tests |
|-------|-----------|-------|
| 6a | Separable blur + shadow props + two-pass renderer | 21 |
| 6b | MSDF atlas (text + icons) | 22 |
| 6c | SDF shapes (rounded rects, circles, panels) | 23 |
| 6d | 9-slice shadow | 8 |
| 6e | Dual-kawase blur | 7 |
| 6f | Render cache + damage integration | 15 |
| Chain | Integration chain (inputd→windowd→fbdevd) | 2 |
| **Total** | | **98** |

## Proofs

```bash
cargo test -p ui_v4_host         # 98/98
cargo test -p windowd            # 31/31
cargo test -p ui_v3b_host        # 20/20
just dep-gate                    # PASS
```
