---
title: TASK-0326 gpud's GL capability is decided when GL is first NEEDED, not at a point in start-up
status: Done 2026-09-19
owner: @gpu
created: 2026-09-19
depends-on: []
follow-up-tasks: []
links:
  - Policy this extends: tasks/TASK-0324-display-handoff-deterministic-by-construction.md (P0, scanout policy)
  - Race-free display mode: docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md
  - Stage ordering: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md
---

## The defect (measured 2026-09-19)

`just start` shows no picture roughly one run in four. The boot does not wedge — gpud EXITS:

```
gpud: features=os-lite,virgl
gpud: FAIL gl draw unavailable on gl device
init: service exit name=gpud reason=error code=0xffffffff
```

The policy that kills it is correct and stays (TASK-0324 P0): a GL device is driven through the
GL render target or not at all, because the 2D plane-row scanout is BLACK on every GL display
backend. Loud death beats a black screen.

The defect is the PROVENANCE of one of its three inputs. `scanout_path(gl_device,
virgl_draw_ok, has_virgl_feature)` treats `virgl_draw_ok` as a property of the DEVICE, but it
is measured once, in `probe()`, as a property of a MOMENT — and that moment is before the host
window is realized. The code two lines below the fatal call already says so, about the very
same backend:

> *"GET_DISPLAY_INFO is validated CAPABILITY only (QEMU's GTK backend **transiently** reports
> the un-realized window default → wrong mode)"*

RFC-0074 closed that race for the MODE: the kernel-derived configured mode is authoritative and
the device's word is evidence. The DRAW capability was left probing the same transiently-absent
backend, and its failure is fatal. One race, one half closed.

**Measured:** in three `just start` runs that produced a picture, the device reported
`640x507` instead of the configured `1280x800` — the un-realized window — and the GL draw
happened to pass. In the fourth it did not, and gpud died. Eight harness lanes (`visible`,
`smp1`, non-GTK backends, no window manager in the loop) never show either symptom.

**Second defect, found while diagnosing.** The capability cascade is SILENT on failure at three
of its four steps: `submit3d_selftest`, `virgl_rt_clear_test` and `virgl_shader_test` each skip
their OK marker and say nothing. When the fatal marker fires there is no breadcrumb saying
which step did not happen — which is why the failing run cannot be attributed to a step today.

### Architecture review (three lenses)

- **Scope** — in: `gpud/src/backend/probe_policy.rs` (defer the decision, breadcrumb every
  failed step), `gpud/src/gl_scanout_init.rs` (the decision point), `gpud/src/markers.rs` +
  the marker manifest. Out: `scanout_policy.rs` (the policy is right and does not change), the
  display-mode handling (RFC-0074 owns it), boot timing, and the folded console's
  lost-on-death buffer (recorded as a follow-up below).
- **Invariant** — a GL device is NEVER driven by the 2D path (unchanged), AND the fatal exit
  is taken only from a decision made while the display is live. "Not proven yet" is not
  "cannot". Negative tests: `test_reject_fatal_before_display_is_live`,
  `test_reject_gl_device_on_2d_path_after_deferral`.
- **Contract** — extends TASK-0324 P0's scanout policy with the race-free rule RFC-0074 already
  applies to the mode. No new ADR: no boundary moves, the same service, the same pure policy
  function.

## Goal (end system)

The GL capability is decided at the moment GL is first NEEDED — `gl_scanout_init`, which by
construction runs after the display is live — not at a fixed point in start-up. A probe that
cannot prove the capability yet records exactly that and says which step it got to; the fatal
exit fires when GL is genuinely unavailable, with the failing step named.

## Non-goals

Changing the scanout policy or its fatal outcome; re-probing forever (one decision, one
moment); the display-mode mismatch; a clock, a timeout or a retry budget anywhere.

## Invariants

- A GL device never reaches the 2D plane-row path. If GL cannot be proven when it is needed,
  gpud exits loudly — it does not present black.
- No clock, no deadline, no retry count: the decision hangs on the first GL need, which is an
  event, not a duration (RFC-0093 §7).
- Exactly one of `gpud: virgl ready` / `gpud: cpu fallback` still, and now exactly one
  breadcrumb per failed cascade step.

## What was built (2026-09-19)

`probe()` no longer decides. `draw_verdict(gl_device, draw_ok, needed_now)` — pure, in
`scanout_policy.rs` beside `scanout_path`, four tests — names the rule: unproven at start-up
DEFERS, unproven when GL is needed is FATAL. At the scanout attach the deferred case simply
ATTEMPTS GL, and the attempt is the proof.

**The design changed once, and the measurement is why.** The first cut re-ran the capability
cascade at the attach seam. A forced-condition experiment showed it cannot work:

```
gpud: virgl draw ok                        <- first cascade: fine
gpud: gl draw unproven at probe (...)      <- deferral fires
gpud: virgl submit3d ok
gpud: virgl rt clear fail                  <- SECOND pass collides with itself
gpud: virgl draw submit fail
gpud: FAIL gl draw unavailable on gl device
```

The cascade's test objects sit on FIXED virgl ids, so re-running it re-creates a render target
that already exists. A self-test is not a thing you can repeat. So the proof became the real
work: `gl_scanout_init()` is attempted, and its existing error path — which already names the
failing class and exits loudly, TASK-0324 P0 — is the loud failure. Nothing new had to be
built for the fatal half; it was already there, one layer down from where the decision sat.

That experiment was only readable because of the second fix in this task: the cascade used to
say nothing when a step failed. Without `virgl rt clear fail` there was no way to see it.

**A hole in the first cut, kept fixed:** the probe cleared `virgl_capable` when the context
could not be created — overwriting what the DEVICE negotiated with the outcome of one probe,
the same conflation this task exists to remove, and it would have made any later proof
impossible. It is not cleared any more; every consumer gates on `virgl_draw_ok` or the context
id as well, so nothing else changes.

## Packages

- **P0** ✅ The rule + the deferral + the breadcrumbs + host tests.
- **P1** ✅ Proof in situ.

## Definition of Done

Host: 8 `scanout_policy` tests, including `test_reject_fatal_before_display_is_live` and
`test_reject_soft_failure_when_gl_is_needed` (the two halves of the rule) and
`test_reject_gl_device_on_2d_path_after_deferral` (deferring must not widen the path).

QEMU, **the deterministic proof**: with the capability forcibly unproven at probe — the exact
state that used to kill gpud — the visible lane runs green end to end:

```
gpud: gl draw unproven at probe (deciding at first scanout)
gpud: gl scanout ok
gpud: gl draw proven at first scanout
SELFTEST: Completed (markers verified)
```

Regression: `just test-all` 27 PASS / 0 FAIL, and **34 `just start`-shaped gtk runs with zero
`init: service exit name=gpud`**.

**Stated limit.** The natural failure did not recur in those 34 runs, so the fix is not proven
against a spontaneous occurrence — only against the forced one. The forced condition is the
same state (`virgl_draw_ok == false` when the scanout policy is applied), which is what makes
it evidence; but the trigger that produces it in the wild is still uncharacterised, and 34
clean runs is a weak sample against a fault that appeared once in roughly six.

### Follow-up (recorded, not done here)

- **The folded console loses a dying service's breadcrumbs.** A group's OK markers are buffered
  and flushed as one verdict line; when the service dies mid-group the buffer is never flushed,
  so the failing run shows the FAIL with nothing before it. That is exactly the evidence a
  post-mortem needs. FAIL markers already bypass folding; the buffered breadcrumbs of a group
  that ends in a service exit should too.
