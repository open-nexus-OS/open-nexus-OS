# Handoff — TASK-0059 Phase 6 visual closure (in progress)

Date: 2026-05-19  
Session: chat work only (debugging paused)

## Status

| Item | State |
|------|--------|
| RFC-0058 Phases 0–6 (libraries, host unit tests) | Largely done before this session |
| Phase 6 **visual** (parent glass, shadow, targets) | Code in working tree, **uncommitted** |
| reactive-budgets + prove-latency-fixes (host) | Addressed in session arc; verify still green |
| `just test-os visible-bootstrap` | **Red** — black screen / missing scanout chain |
| `just start` | **Black** — same |

**Do not mark TASK-0059 visual closure done.**

---

## This chat — completed work

### User-visible goals

- Parent panel: dark translucent background + blur + box shadow (children unaffected).
- Fix regressions: stripe shadows, opacity on wrong nodes, slow target color “waves”, scroll clip, black screen / no first frame.

### Code changes (uncommitted)

1. **`layout_panel.rs`** — glass on `combined_panels` only; child panels plain; scroll layout tweaks; host layout tests updated.
2. **`os_lite.rs`** — alpha `fill_row_rect`; segment + vertical shadow; multi-range damage; wheel summary marker latch; budgeted first-frame glass quality; target damage via row bands.
3. **`windowd/Cargo.toml`** — `stack_pages = 8`.
4. **`interactive_os_startup.rs`** — host guards for wheel latch, first-frame glass policy, stack pages.

### Marker / harness (earlier in same chat arc)

- Moved summary markers toward service-owned emission (`windowd`, `fbdevd`).
- `visible-bootstrap` proof-mode auto-exit intent.
- Wheel ordering bug identified: emit summary only after `visible_input_summary` **and** latched wheel hop.

### Experiments that did not stick

- `BACKDROP_CACHE_MAX_WIDTH` = full combined width (~826px) → boot/wiring instability; reverted.
- Rect damage fast path for targets → reverted to row bands (slower but wrong path was worse).

---

## This chat — QEMU evidence

**User:** screen stays black on `visible-bootstrap` and `start`.

**Typical UART ladder break:**

```
fbdevd: ready
fbdevd: map ok
fbdevd: ramfb configured
… (no fbdevd: flush ok)
… (no windowd: present visible ok)
inputd: windowd visible-state push fail
```

When `windowd` runs but does not flush: `compose_hz=0`, `damage_px=0`.

**Conclusion for next agent:** fix **first frame handoff** (`fbdevd` ↔ `windowd` `register_framebuffer` / `write_current_frame`) before more visual tuning.

---

## Host tests vs QEMU

Host green ≠ OS green. Specific gaps:

- String-presence tests do not prove marker **order** or **runtime** conditions.
- No test that first full frame write succeeds for combined-panel layout size.
- Two `nx` tests failed on broader filter — may assert outdated implementation strings.

Added in this session (good):

- `windowd_latches_wheel_hop_before_visible_input_summary`
- `windowd_first_frame_uses_budgeted_glass_quality`

---

## Files touched (git)

```
source/services/windowd/src/layout_panel.rs
source/services/windowd/src/os_lite.rs
source/services/windowd/Cargo.toml
tools/nx/tests/interactive_os_startup.rs
neuron-boot.map   # verify separately
```

---

## Suggested next steps (when resuming)

1. Reproduce one QEMU run; capture **first** missing marker only.
2. Trace `register_framebuffer_with_windowd` → `DisplayServerRuntime::register_framebuffer` → `write_current_frame` (log on failure, no silent `STATUS_MALFORMED`).
3. Host test: one-row render at combined panel width with opacity (catch cache/layout errors).
4. Only after scanout works: validate glass look + instant targets on device.
5. Reconcile stale `nx` contract tests.

---

## Related

- Full detail: `.cursor/current_state.md`
- Task/RFC: `tasks/TASK-0059-*`, `docs/rfcs/RFC-0058-*`
- Transcript: agent session `8ea8ef57-5e45-4891-8dcc-a321a3a6e731`
