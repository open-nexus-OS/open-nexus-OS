<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0083: The compositor states what is seen, the lifecycle authority decides, the parent orders — a hidden app is frozen after a grace and thawed when it is seen again

- Status: Accepted (2026-10-27; the proof is the visible QEMU lane and the board cycle of TASK-0333 6d)
- Date: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-2)
  - RFCs: `docs/rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md`
    (§4 the states, §5 the wire — this decision fixes the userspace half), `docs/rfcs/RFC-0065-…`
    (abilitymgr is the lifecycle authority and the only app spawner), `docs/rfcs/RFC-0086-…`
    (the window feed: the slot facts the sightings are built from)
  - Related ADRs: `docs/adr/0082-…` (the kernel half: the parent freezes, thaws and kills the
    process whole; `Frozen` refuses the park), `docs/adr/0073-…` (death is an event: the death
    report that ends a thawed-to-exit instance), `docs/adr/0075-…` (the compositor never waits
    on a client — nor on the authority), `docs/adr/0036-…` (lifecycle vs process vs registry)

## Context

After ADR-0082 the kernel can freeze a process on its parent's order, but nobody orders. Three
services know a different third of the truth: the compositor knows what is on screen, the
lifecycle authority (abilitymgr) knows which instance is which app and in which state, the
service launcher (execd) is the parent the kernel accepts orders from. Measured on the board
through 6b–6c: a hidden app keeps its share of the CPU (its frame loop, its timers), and in a
four-window session that share is the foreground's judder. The phones this OS is measured
against froze the hidden app a decade ago: the compositor is not the authority, the authority
is not the parent, and the process never gets a vote.

Three designs were on the table: the compositor orders the kernel directly (fast, and a second
lifecycle authority), the app parks itself when told (cooperative, and a spinning app keeps
spinning), or the three services each state what they know and ONE decides.

## Decision

- **The compositor STATES, it never decides.** For every app window windowd publishes what is
  seen — `Focused`, `Visible`, `Hidden` (minimized), `Gone` (closed) — as `OP_SEEN` to
  abilitymgr, keyed by the kernel service id of the window's process (`app:<bundle_id>`),
  derived from the same slot facts as the shell's window feed, deduplicated against the last
  sighting the authority took, NONBLOCK and owed-retried like the feed itself. windowd never
  learns whether an app is frozen and never needs to: a send into a frozen app's full queue
  (`Frozen`) is a full queue to it, drained by the thaw.
- **The authority DECIDES, on one clock.** abilitymgr maps a sighting to the instance's
  lifecycle state (RFC-0111 §4): on screen is `Foreground` or `Background`; hidden starts the
  grace (5 s, one constant in `seen.rs`); the grace out is `Suspended`; a suspended instance
  seen again is thawed into the state it is seen in; one whose window is gone is thawed so it
  can exit. The clock is a one-shot kernel timer on a declared pair, armed at the earliest grace
  to run out; the loop waits on its server endpoint and the timer, nothing else, and a request's
  reply is sent on its own so no receive is ever a blocking one while a grace waits.
- **The parent ORDERS.** abilitymgr asks execd (`OP_FREEZE` / `OP_THAW` / `OP_KILL`) and
  execd, the parent, issues `task_suspend` / `task_resume` / `task_kill` — after checking the
  sender is abilitymgr (kernel sender id) and the pid is a LIVE child; an ordered kill is
  recorded before the order so its reap is not a crash (ADR-0082).
- **The hide is the user's; the keyboard has one key for it.** Super+Ctrl+M minimizes the
  focused app window; on the bare desktop it restores the first hidden one (the taskbar's
  fly-in). The chord is inputd's one-shot fact and windowd's `CODE_MINIMIZE` (14), the verb's
  codes stay append-only.
- **The proof** is the cycle on the visible lane and the board: `windowd: minimize id=app` →
  `execd: freeze (pid=` → `abilitymgr: suspend (inst=` → restore → `execd: thaw (pid=` →
  `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` (once per boot, on the first full
  cycle); the refusals are host tests (`test_reject_seen_from_anyone_but_the_compositor`,
  `test_reject_order_from_a_stranger_or_for_a_stranger`).
- **Out of scope here, by decision:** occlusion (a window fully covered by another is
  `Visible`; the occlusion sighting is a follow-up with its own measurement); the ghost band of a
  system-initiated kill (6e: there is no system kill yet, a crash closes the window as before);
  `OP_KILL` from the authority (the op and execd's side exist; the policy that issues it is
  6e's pressure band); the `background` grant that exempts an app from the freeze (6d-3).

## Consequences

- **Positive**: a hidden app costs the foreground nothing — not by its cooperation but by the
  kernel's freeze on the authority's decision; one lifecycle authority (no second one in the
  compositor); the compositor stays a compositor (ADR-0075: it never waits on the authority,
  never on a frozen app); the whole chain is one deterministic proof on every visible lane.
- **Negative / accepted cost**: a frozen app's queued frames wait in its windowd outbox for
  the thaw (bounded; a far-behind frozen app's ordered frames are dropped and counted, as any
  far-behind client's); a 5 s grace is a constant, not a per-app setting (6d-3 adds the one
  exemption that matters, the declared `background` grant); abilitymgr gains a timer pair and a
  waitset — the declared-slot cost every clocked service pays.
- **Follow-ups**: 6d-3 (the grant at policyd); 6e (the pressure bands that issue `OP_KILL`,
  the ghost band); occlusion as a sighting; a window-only instance (no process) moves through
  the same states and orders nothing — it is removed with its placeholder apps.

## Alternatives considered

- **The compositor orders the kernel directly.** Rejected: a second lifecycle authority, and
  windowd would need to be every app's parent to be heard by the kernel (ADR-0082). The
  compositor stating and the authority deciding keeps RFC-0065's one authority.
- **Cooperative sleep (the app parks when told).** Rejected as the mechanism (ADR-0082): the app
  that misbehaves is exactly the one that must be stopped. Kept as a courtesy a later
  `OP_CLOSE` may add before a kill.
- **A per-request transition op (`OP_TRANSITION(instance, to)`) from the compositor.** Rejected:
  the compositor would have to know instance ids and lifecycle states; it knows windows and
  what is seen of them. The sighting is the honest datum; the state is the authority's.
- **Freeze at once on hide, no grace.** Rejected: a write in flight, a reply the app owes
  another service — a few seconds cost nothing (the kernel's freeze is the floor, not a race)
  and spare the user a half-saved document. The grace is one constant, measured, not a knob.
