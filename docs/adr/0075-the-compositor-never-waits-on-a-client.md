<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0075: The compositor never waits on a client — every delivery is non-blocking, what does not fit waits in a bounded outbox, and the kernel says when a client's queue has room

- Status: Accepted
- Date: 2026-10-21
- Links:
  - Tasks: `tasks/TASK-0339-the-compositor-never-waits-on-a-client.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md` (the writable
    member — this ADR's kernel half), `docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md`
    (the input budget this unblocks), `docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md` (§7: the
    delivery classes this replaces)
  - Related ADRs: `docs/adr/0042-cross-process-surface-transport.md` (the per-app event
    channel), `docs/adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md`
    (a send to a dead client is `PeerClosed`)
  - Measurement: `docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md` ("Cycle 54")

## Context

Cycle 54 removed the display ring's overflow and the input waits fell 3.7× — to 0.3–1.5 s at the
worst frame of each window, while windowd's loop ran 50–180 times a second and presented 1–9
times; the slow seconds sat at the app launches. The code says why: every frame windowd sends a
client for which "a client waits on it" (a present, create or destroy ack) or "nothing
reconstructs it" (a tap, a key, a text commit, a settings push — and, by accident, the scroll
position) was sent PARKING (`Delivery::Blocking`/`Critical`, RFC-0093 §7): the kernel queued
windowd as a send-waiter until the client drained. An app's event channel holds eight frames;
the pointer moving over a launching window fills it with hover moves in a tenth of a second;
the app renders its first frame for half a second on the board's CPU and drains nothing. The
next ack or tap parks windowd for the rest of that render — no frame, no pointer, no other
window. The presentation pump, the one path that did not park, retried its snapshot once a frame
and gave up after eight refusals, losing a theme change.

The reference class never lets its compositor wait on an application: Android's input
dispatcher keeps an outbound queue per connection and waits for the socket to become writable;
Fuchsia's channels signal WRITABLE; the window server of the other phone coalesces events per
app and marks an unresponsive one. The kernel here could wake a parked sender when a queue got
room, but a waitset — the one thing windowd waits on — could only report a message, not room.

## Decision

- **windowd never waits on a client.** Every frame for an app or the desktop goes through one
  funnel (`client_send`) that sends non-blocking. When nothing of that client waits before it,
  the frame goes straight into the client's queue; otherwise it waits in the client's outbox.
- **Two kinds of frame, decided by what the client needs, not by how hard to try.**
  ORDERED — an ack the client waits for (`Delivery::Ack`), user intent with no recovery path
  (`Delivery::Intent`: a tap, a release, a wheel step, a key, a text commit, a window rect, a
  settings push) — is delivered in order, never merged, never dropped while the client lives.
  LATEST (`Delivery::Latest`) — state the next frame supersedes: the pointer over the window, a
  drag's position, the scroll position, a frame pulse, a presentation snapshot, a content rect —
  keeps one pending frame per kind, the newest replacing the older. Frames leave in the order
  they were queued, a LATEST frame at the place of its last update: a client never sees a state
  older than an event queued after it.
- **The outbox is bounded and allocated once.** Sixteen ORDERED frames and eight LATEST kinds per
  client, a pool of eight clients (the event-channel table's bound), allocated when the compositor
  starts and reused; a client with
  sixteen ORDERED frames waiting is far behind: a further one is refused and counted, and the
  client named on the loop line (`clients(behind= queued_max= dropped=)`). What to do with a hung
  app is the lifecycle authority's decision (layer 6d/6e), never the compositor's park.
- **The kernel says when a queue has room (RFC-0033 amendment).** A SEND-only capability added to
  a waitset is a WRITABLE member, ARMED: ready once the queue has room or no receiver remains,
  reported once (the report disarms it); adding it again re-arms it. Every receive wakes the
  endpoint's writable waiters beside its first parked sender — from a list apart, so a waitset
  never takes a parked sender's wake. windowd arms the member of a channel whose queue was full
  (`waitset_arm_writable`), waits on its one waitset as before, and sends what waits on every
  loop pass. No polling, no timer, no spin on a client with room it has nothing for. A byte-budget
  refusal (the queue has room, the receiver's bytes do not) waits for the next pass instead — the
  writable member reports queue room only.
- **A client's death ends its outbox.** A send that finds no receiver is ADR-0073's `PeerClosed`:
  what waited is dropped and the window closes by name (`windowd: app died (slot=`). A closed or
  re-attached channel drops its outbox with it; a reused capability slot never receives a dead
  client's frames.
- **Out of scope here:** app-host still waits on windowd for its own sends (backpressure toward
  the compositor; no deadlock remains, because windowd never waits); the lifecycle authority's
  response to a hung app; other services' pushes to apps (settingsd's) use the same kernel signal
  when they move to an outbox.

## Consequences

- windowd's loop keeps composing while an app renders: the pointer, the other windows, the
  animations go on. A busy app gets its frames late, in order, the newest state for each kind.
- An ack may reach a busy client later than before; the client was not reading anyway.
- The presentation pump's per-frame retry and its give-up after eight refusals are gone: a
  snapshot is delivered once the client drains.
- The kernel gains one member kind and no syscall: `waitset_add` with a SEND-only capability,
  which was refused before (`RECV` was required).
- Memory: the outbox pool, ~6.75 KiB per client (24 slots of the largest event frame), ~54 KiB
  allocated once from windowd's 2 MiB heap, never per frame.

## Rejected alternatives

- **A deeper event channel.** A busy client fills any depth with hover moves; the next ack still
  parks the compositor. Depth only moves the cliff.
- **A user-space credit or pull protocol** (the client tells windowd what it consumed). Correct,
  but per protocol: every server that pushes to apps would need its own, and the client's
  silence while it renders is exactly when the credits stop.
- **Retrying on the frame clock.** Polling in disguise (the reactivity rule): it costs a present
  per retry and still waits up to a frame.

## Proof

- Host: `client_outbox` (order across both kinds, the merge, a full client, a dead client, a
  byte-budget stop, `test_reject_an_ordered_frame_past_the_bound`,
  `test_reject_a_frame_longer_than_any_event_or_empty`), the delivery classes
  (`test_reject_merging_a_release_or_a_wheel_step`), the kernel's member word
  (`a_writable_member_is_armed_once_and_rearmed_in_place`,
  `test_reject_an_endpoint_id_in_the_flag_bits`).
- Every boot: `KSELFTEST: waitset writable ok` (armed while full: not ready; room: ready once;
  re-armed: ready; the receiver gone: ready) on the QEMU ladders.
- Board: `windowd: ux input` at the app launches against cycle 54, `clients(behind=` on the loop
  line.
