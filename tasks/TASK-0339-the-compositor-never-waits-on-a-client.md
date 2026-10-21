---
title: TASK-0339 The compositor never waits on a client — non-blocking delivery, a bounded per-client outbox, the kernel's writable signal, and windowd's own work measured
status: In Progress (built 2026-10-21; host, kernel, `just check`, `just diag`, QEMU headless + visible green; board cycle 55: no park, the work split measured — the commit next)
owner: @ui @kernel-team
created: 2026-10-21
depends-on: [TASK-0338 (one frame producer — cycle 54), TASK-0333 6a (a send to a dead client is `PeerClosed`)]
follow-up-tasks: [app-host never waits on windowd (the other half of the plan's 6d line), the lifecycle authority's answer to a hung app (6d/6e), settingsd's pushes on an outbox, the atlas copy without syscalls per row (by cycle 55's `work(copy_` numbers)]
links:
  - ADR: docs/adr/0075-the-compositor-never-waits-on-a-client.md
  - RFC: docs/rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md (amendment: the writable member)
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md (the input budget)
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 54")
  - Code: source/services/windowd/src/{client_outbox.rs,client_surface.rs}, source/services/windowd/src/compositor/runtime/client_send.rs, source/kernel/neuron/src/{waitset.rs,ipc/eof.rs,syscall/api/sync_objects.rs,selftest/lifetime.rs}
---

## Context (cycle 54)

- The ring overflow was gone (0 NACKs) and the worst input wait per 16 frames fell to a median of
  833 ms (max 1.5 s) — still far over RFC-0108's 50 ms. windowd's loop ran 50–180 times a second
  and presented 1–9 times; the slow seconds (`hz=5–8`) sat at the app launches.
- In the code: every ack, tap, key, text commit, settings push — and the scroll position — went to
  the client's 8-deep event channel PARKING (`Delivery::Blocking`/`Critical`). Hover moves fill
  that channel within a tenth of a second while an app renders its first frame (0.5–1 s on the
  board); the next ack or tap parked windowd for the rest of the render: no frame, no pointer, no
  other window. The presentation pump retried its snapshot per frame and gave up after eight.
- Also measured as missing: no line splits windowd's own CPU work per frame from its waiting —
  and two suspects of that work: each app present is copied into the atlas with two syscalls per
  row (a scrollable app's whole tall band, every present), and every content rectangle rewrites
  the wallpaper into the retained plane with a syscall per row (read on the CPU executor only).
- `apphost: ux launch` was cut at 95 characters, before its `budget_ms=` (RFC-0108's format).

## Goal

windowd never waits on a client, and every run splits the input's wait into "windowd works" and
"windowd waits", so the next cut is chosen from numbers.

## Non-Goals

- app-host never waiting on windowd (no deadlock remains; the backpressure direction stays).
- The lifecycle authority's answer to a hung app (6d/6e).
- Cutting the atlas copy or the retained-plane rewrite (measured first, here).

## Three lenses

- **Architect:** ADR-0075 (the delivery contract) + an RFC-0033 amendment (the writable member).
  No new syscall: `waitset_add` with a SEND-only capability, refused before, now arms a sender's
  member; nexus-abi names it `waitset_arm_writable`.
- **Security:** a SEND-only capability can only arm a wake for its own holder (the waitset's
  owner check stands); the member word refuses an endpoint id in its flag bits
  (`test_reject_an_endpoint_id_in_the_flag_bits`); the outbox is bounded per client and
  refuses past it by name (`test_reject_an_ordered_frame_past_the_bound`), a frame longer than any
  event frame is refused (`test_reject_a_frame_longer_than_any_event_or_empty`); a merged
  release or wheel step is impossible (`test_reject_merging_a_release_or_a_wheel_step`).
- **Pragmatist:** one funnel replaces the old one (`client_send`), the outbox is a pure module
  with host tests, the kernel change reuses the send-waiter wake. Not doing: the cuts the new
  lines will motivate.

## Plan / what was built (2026-10-21)

- **Kernel (RFC-0033 amendment):** the waitset member word (`MEMBER_WRITABLE`, `MEMBER_ARMED`,
  `arm_writable`, `disarm`, `forget_endpoint` on the endpoint part); `sys_waitset_add` arms a
  sender's member for a SEND-only capability; `sys_waitset_wait` scans a sender's member as
  "armed and the queue has room" (`Router::writable`: room, or no receiver), registers it in the
  endpoint's writable-waiter list (apart from its parked senders), disarms it when it reports it;
  every receive wakes the first parked sender and every writable waiter (`wake_after_recv`, the
  four receive sites), an endpoint's death or its receiver's departure wakes them too; the
  deadline path deregisters both interests (`deregister_members`). Boot proof
  `KSELFTEST: waitset writable ok` (armed while full, room, spent, re-armed, receiver gone, and a
  receive waking a parked sender and a writable waiter apart).
- **windowd (ADR-0075):** `client_outbox` (ORDERED ring of 16, LATEST slots of 8 keyed by op and
  input group, merged by sequence; a pool of 8 — the event-channel table's bound — allocated once, ~54 KiB); `client_send` — straight into the
  queue when nothing waits, else into the outbox, flushed every loop pass, the writable member
  armed on a full queue (not on a byte-budget refusal, which waits a pass), the outbox dropped on
  a client's death or a channel's close/re-attach. `Delivery` is `Ack`/`Intent`/`Latest` (a wheel
  step is intent now, the scroll position and the content rect are state). Every send site uses
  the funnel; the pump's snapshot is state.
- **Lines:** `windowd: loop …` gains `clients(behind= queued_max= dropped=)` and
  `work(flush_us_max= copy_rows= copy_us= base_rows= base_us=)`. app-host's marker buffer holds
  192 bytes: `apphost: ux launch` whole.

## Proof

- Host: `client_outbox` (10), the delivery classes (3), the kernel's member word (3 new); windowd
  201, kernel 134.
- QEMU `headless`, `visible`: `KSELFTEST: waitset writable ok` in the ladder's order.
- Board cycle 55: `windowd: ux input` at the launches against cycle 54; `clients(behind=` shows
  how often a client was behind; `work(` splits windowd's own time.

## Results

- 2026-10-21: built; host and kernel tests, `just check`, `just diag`, `build-os-workspace`
  (0 warnings) green; QEMU `headless` (twice, the final tree) and `visible` green with
  `KSELFTEST: waitset writable ok` in its place.
- **Cycle 55 (board, 2026-10-21):** the operator: "a little better again". No park any more: a
  client was behind in 12 of 147 one-second windows (at most one frame waiting, none dropped) —
  each a park before. The work split (`work(…)`): windowd spent 10.2 s rewriting 84 561 wallpaper
  rows into the retained plane (120 µs a row, 95–220 ms per window-sized frame) and 1.6 s copying
  42 081 app rows into the atlas (37 µs a row); the longest flush 254 ms. `windowd: ux input` stays
  red (median of the window maxima 1.15 s) — it is windowd's own work now, not waiting: the next
  package cuts the retained-plane rewrite where the GPU composes and the per-row syscalls of the
  atlas copy. Acks: desktop, pointer, gpu-desktop.
