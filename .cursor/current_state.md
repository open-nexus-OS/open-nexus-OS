# Current State — Open Nexus OS

Last updated: 2026-05-19 (this chat session — visual/QEMU **not** closed)

## Active focus

**TASK-0059 Phase 6 — visual polish + marker chain + host proofs.**  
Debugging **stopped** on user request; only `.cursor/` docs updated.

---

## What this chat set out to do

1. Fix UI regressions after Phase 6: parent panel with opacity + blur + shadow (Tailwind-like), not on child cards; text/icons/SVG should respect effects.
2. Fix delayed first frame, key-press after click, lag / slow target color changes, scroll panel clipping.
3. Complete todos **reactive-budgets** and **prove-latency-fixes** with **host tests first**, then QEMU.
4. Make `just test-os visible-bootstrap` auto-exit after tests.

---

## Implemented in working tree (uncommitted)

### `source/services/windowd/src/layout_panel.rs`

- Moved glass treatment to **`combined_panels` only**: opacity (~132/255), blur, `panel_shadow()` (offset_y 12, blur 24, spread 4).
- Child panels (`proof_panel`, `filter_panel`) use `VisualStyle::default()` — no extra opacity/shadow.
- Scroll card: smaller scroll markers, tighter padding/gap to reduce top/bottom clipping.
- Host tests updated: `combined_parent_uses_translucent_blurred_shadowed_backdrop`, `child_panels_do_not_apply_extra_opacity_or_shadow`.

### `source/services/windowd/src/os_lite.rs`

- **`fill_row_rect`**: alpha blending instead of `copy_from_slice` (opacity actually visible).
- **`compute_shadow_row`**: vertical alpha falloff + blur only shadow segment (not full row); removed blanket `shadow_scratch` zero at row start.
- **Damage**: `pending_damage_rows` as up to 4 ranges; cursor old/new queued separately (not one merged span).
- **Target updates**: `queue_target_damage` uses row bands again (rect fast path removed from hover/click/key — was slower in practice).
- **Markers**: wheel summary `SELFTEST: ui visible wheel ok` uses latched `input_markers_emitted.wheel`, not transient `wheel_up/down` at emit time (fixes “wheel visible” before “visible input ok” ordering bug).
- **First frame**: `write_current_frame` uses `select_glass_quality(self.mode.height)` instead of forced `GlassQuality::High` (800-row high blur blocked boot).
- Tried widening `BACKDROP_CACHE_MAX_WIDTH` to full combined width (~826px) → correlated with init/`windowd` wiring failures; **reverted** to `PANEL_WIDTH` (610) only.

### `source/services/windowd/Cargo.toml`

- `stack_pages = 8` (was 4) — larger `DisplayServerRuntime` on OS stack.

### `tools/nx/tests/interactive_os_startup.rs`

- `windowd_latches_wheel_hop_before_visible_input_summary` — wheel summary latch contract.
- `windowd_first_frame_uses_budgeted_glass_quality` — first frame must not force High blur on full height.
- Assert `stack_pages = 8` in windowd metadata.
- Earlier in chat (same branch): `windowd_target_color_changes_use_single_row_band_fast_path`, chain/marker contract tests referenced in session.

### Other files in `git status`

- `neuron-boot.map` — large regen; likely build artifact, **not** part of visual logic — confirm before commit.

### Earlier chat work (may already be on branch before this session’s diff)

From session arc (not all re-listed in latest `git diff --stat`):

- Marker emission fixes (`selftest-client`, `fbdevd`, `windowd` observer latch).
- `reactive-budgets` / `prove-latency-fixes` host-side work.
- `LayoutHotPathIndex`, `DamageRect`, `live_runtime` helpers.
- Alloc reductions (`blur_row_buf`, fixed arrays).

---

## Host test status

| Check | Result |
|-------|--------|
| `cargo test -p nx windowd_latches_wheel_hop_before_visible_input_summary` | OK |
| `cargo test -p nx windowd_first_frame_uses_budgeted_glass_quality` | OK |
| `cargo test -p windowd` (host) | OK in session |
| `just dep-gate && just diag-os` | OK in session |
| `cargo test -p nx windowd_` (broader) | **2 failures** — stale string contracts: `windowd_keeps_cursor_motion_out_of_layout_recompute_hot_path`, `fbdevd_polls_windowd_with_owned_cap_move_reply_inbox` |

**Lesson:** green host unit/contract tests did **not** predict QEMU black screen.

---

## QEMU / evidence (this chat)

User report: **`just test-os visible-bootstrap` and `just start` stay black.**

Observed across runs (UART / agent logs):

| Symptom | Interpretation |
|---------|----------------|
| `fbdevd: ramfb configured` but **no `fbdevd: flush ok`** | `register_framebuffer_with_windowd` never succeeds → no initial `windowd` frame in VMO |
| Timeout on **`windowd: present visible ok`** | No display bootstrap / first scanout |
| `inputd: windowd visible-state push fail` | `windowd` not reachable on expected route |
| `windowd: route fallback` only (no `windowd: ready` in bad runs) | IPC slot/bootstrap issue or service not running |
| `fps: windowd compose_hz=0`, `fbdevd flush_hz=0`, `damage_px=0` after input | No dirty flushes after boot — consistent with black screen |
| Missing `SELFTEST: ui visible wheel ok` (earlier) | Fixed in tree via wheel latch; not yet proven on QEMU |
| Intermittent `init: capability-denied` at `wire svc=windowd` | `windowd` not wired; separate from render bugs |

Evidence artifacts: `target/evidence/*visible-bootstrap*.tar.gz`, agent logs under `.cursor/projects/.../agent-tools/*.txt`.

---

## Host tests: gaps / wrong assumptions

**What host tests falsely implied “done”:**

- Grep-only tests that `debug_println(SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER)` exists in source — **not** that OS emits it after `visible_input_summary` with wheel latched.
- `ui_v4_host` / `windowd` unit tests — no `register_framebuffer` → `write_current_frame` → `vmo_write` path.
- Chain tests — host memory, not OS caps/timing/stack.

**What to add before trusting host again:**

1. Host render/simulate one combined-panel row at 826px with opacity — must not error (`BufferLengthMismatch`).
2. Contract: `register_framebuffer` failure path logs stable label; success requires non-empty scanout bytes.
3. Paint-only toggle (hover) must not schedule full-height shadow pass (behavior test, not string grep).
4. Reconcile or fix the two failing `nx` `windowd_` / `fbdevd_*` contract tests.

---

## Not done / still open

- [ ] QEMU green: `fbdevd: flush ok` → `windowd: present visible ok` → visible-bootstrap ladder including auto-exit.
- [ ] Visual validation on device: parent glass, instant targets, scroll not clipped.
- [ ] Confirm `neuron-boot.map` intentional.
- [ ] Commit strategy: split visual vs marker vs test-harness changes.

---

## Resume commands

```bash
cargo test -p nx windowd_latches_wheel_hop_before_visible_input_summary windowd_first_frame_uses_budgeted_glass_quality -- --nocapture
cargo test -p windowd
just dep-gate && just diag-os
just test-os visible-bootstrap   # truth test — currently red
```

Handoff detail: `.cursor/handoff/current.md`
