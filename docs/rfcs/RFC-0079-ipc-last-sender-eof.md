# RFC-0079: IPC last-sender EOF (opt-in receiver disconnect)

- Status: Complete
- Owners: @kernel-ipc-team / @runtime
- Created: 2026-07-23
- Last Updated: 2026-07-23
- Links:
  - Tasks: `tasks/TASK-0301-ipc-last-sender-eof.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`
    (the process-image reclaim this EOF finally triggers), `docs/adr/0001-runtime-roles-and-boundaries.md`

## Status at a Glance

- **Phase 0 (kernel EOF contract + opt-in recv + cap-close wake)**: ✅
  (2026-07-23 — `IPC_SYS_EOF` + `PeerClosed`/EPIPE, `had_sender` latch,
  `endpoint_send_cap_count` scan, cap-close wake; the pure decision predicate
  `ipc_eof::should_disconnect` is host-tested with the full reject matrix;
  ci-os-smp1 green — every non-opted server recv blocks as before.)
- **Phase 1 (app-host self-exit on event-channel EOF)**: ✅
  (2026-07-23 — app-host recvs its event channel with `IPC_SYS_EOF` and
  self-exits on `Disconnected`; two cap-leak fixes were required so the
  last-sender scan can actually reach zero: execd now closes its COPIED
  grant clones (`grant_clone`), and app-host closes its own leftover SEND
  clone after attaching it to windowd. Visible boot: every window close
  produces exactly one app exit (1:1), the image returns to the arena
  (RFC-0075 8f), and an open/close storm keeps the arena bounded — zero
  `VMO-POOL exhausted`.)

Definition: “Complete” = the contract is defined and the proof gates are green
(host reject-matrix + QEMU: a closed app window terminates its process and its
image returns to the arena).

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution + proofs live in TASK-0301.

- **This RFC owns**:
  - The kernel recv EOF contract: when a receiver OPTS IN, an endpoint that has
    had a sender and now has none returns a distinct `PeerClosed` error instead
    of blocking forever.
  - The opt-in mechanism (a recv sys-flag), the wake-on-last-sender-close hook,
    and the fail-safe rule (a missing sender-latch or a missed hook must never
    wrongly EOF — it may only fail to EOF).
- **This RFC does NOT own**:
  - A cross-task terminate / kill syscall (reaper #29 — a policyd-gated
    capability; explicitly a separate design). This RFC lets a process
    terminate ITSELF on EOF, which needs no new privilege.
  - The process-image arena reclaim (RFC-0075 8f, already landed) — this RFC
    only makes it FIRE for parked GUI apps by giving them an exit trigger.

### Relationship to tasks (single execution truth)

- TASK-0301 defines the stop conditions and proof commands.

## Context

Closing an app window frees windowd's own slots but does NOT terminate the app
process: the app-host blocks forever on its event-channel `recv` with no sender
(windowd `cap_close`s its SEND cap on close, but the endpoint itself stays
alive). The parked process keeps its ~14 MB image; each new launch spawns a
fresh one. RFC-0075 8f made task exit return the image to the arena — but a
parked app never exits. The missing piece: a way for the app-host to learn
"my last sender is gone" and exit itself.

The kernel today has no sender-side disconnect signal: `recv` on an empty,
still-alive endpoint blocks unconditionally, whether or not any sender exists.
A blanket "no senders → EOF" rule is UNSAFE — every server legitimately blocks
before its first client connects, and would be killed at boot.

## Goals

- A receiver can OPT IN to EOF: `recv` returns `PeerClosed` when the endpoint
  has had ≥1 sender and now has none, instead of blocking.
- A closed app window terminates the app-host process (self-exit), so its
  image returns to the arena (RFC-0075 8f loop closed) — no per-launch leak.
- Zero regression for every existing (non-opted) `recv`.

## Non-Goals

- Cross-task terminate / signals / kill (reaper #29, policyd-gated) — separate.
- A running per-endpoint sender refcount maintained at every cap lifecycle
  event (error-prone; a mis-balanced counter could wrongly EOF). This RFC uses
  a scan + monotonic latch instead (fail-safe).
- Sender-side EOF (a sender learning the receiver is gone) — not needed here.

## Constraints / invariants (hard requirements)

- **Opt-in only**: EOF fires ONLY when the receiver passes the EOF recv flag.
  Every current `recv` is unaffected (server endpoints never EOF).
- **Fail-safe direction**: the failure mode of any missed hook/latch is "does
  not EOF" (the leak persists), NEVER "wrongly EOF" (a live receiver killed).
  Concretely: EOF requires BOTH `had_sender` (a monotonic per-endpoint latch,
  set only when a sender was actually observed) AND a fresh scan proving zero
  live SEND caps to the endpoint across all task tables.
- **Self-exit only**: the app-host calls its OWN `exit` on EOF. No task may
  terminate another — that needs the policyd-gated reaper (#29), out of scope.
- **Bounded**: the sender scan is `O(tasks × slots)` (~30 × 64), run only on
  the recv-would-block path and on cap-close — human-rate for GUI channels
  (mirrors the `vmo_overlap_count` all-tables scan precedent). No new heap.
- **Security floor**: identity stays `sender_service_id`; EOF carries no
  payload and leaks nothing about the sender. A receiver cannot use EOF to
  probe endpoints it lacks a RECV cap for (the flag rides an authorized recv).

## Proposed design

### Contract / interface (normative)

- **Kernel error**: new `ipc::IpcError::PeerClosed`; recv-result errno `32`
  (EPIPE — "broken pipe / peer gone"). `nexus-abi` decodes it to
  `IpcError::PeerClosed`; `nexus-ipc` surfaces it as the existing
  `IpcError::Disconnected` ("opposite endpoint disconnected").
- **Recv opt-in flag**: `IPC_SYS_EOF = 1 << 2` (kernel + `nexus-abi`). Passed
  on a blocking recv; ignored (no effect) on a recv that finds a message.
- **Endpoint state**: `Endpoint.had_sender: bool` — a monotonic latch set when
  a sender is observed (a successful send to the endpoint, or a recv-block scan
  that finds ≥1 live SEND cap). Never cleared. An endpoint that never had a
  sender can never EOF.
- **EOF decision** (recv, would-block, EOF-opted): if
  `had_sender && send_cap_count(ep) == 0` → return `PeerClosed`; else block as
  today. `send_cap_count` = live SEND-right `Endpoint(ep)` caps summed over all
  task cap tables (`CapTable::endpoint_send_cap_count`, mirroring
  `vmo_overlap_count`).
- **Wake hook**: `cap_close` of an `Endpoint(id)` SEND cap, when it drops the
  live SEND-cap count to 0, wakes `id`'s recv-waiters — a blocked EOF-opted
  receiver re-runs recv, re-scans, and returns `PeerClosed`.

### Phases / milestones (contract-level)

- **Phase 0**: kernel EOF contract — the flag, the latch, the scan, the recv
  decision, the cap-close wake, and the ABI error plumbing. Proof: host
  reject-matrix (below).
- **Phase 1**: app-host event loop opts in and self-exits on `Disconnected`;
  the freed image returns to the arena. Proof: QEMU — a closed app window
  emits the app's exit + `IMAGE-RECLAIM`, and an open/close storm keeps the
  arena bounded.

## Proof / reject matrix (Phase 0, host)

- A recv WITHOUT `IPC_SYS_EOF` on a sender-less alive endpoint BLOCKS (never
  `PeerClosed`) — the server-at-boot safety case.
- An EOF-opted recv on an endpoint that NEVER had a sender BLOCKS.
- An EOF-opted recv AFTER a sender existed and all SEND caps closed →
  `PeerClosed`.
- A pending message wins over EOF (deliver the message even if senders are
  gone — draining a closed channel's backlog).
- Closing a NON-last SEND cap does not EOF (other senders remain).

## Follow-ups

- Sender-task EXIT (vs. explicit `cap_close`) that strands a blocked
  EOF-receiver: covered lazily by the next block's scan, but not woken
  promptly. A wake hook in the exit path is a bounded follow-up (windowd, the
  real event-channel sender, is long-lived and uses `cap_close`, so this is
  not on the critical path).
- Reaper #29 (policyd-gated cross-task terminate) remains the mechanism for
  killing a WEDGED app that ignores EOF.
