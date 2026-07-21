---
title: TASK-0299 Time sync v1 (seed): SNTP client in time-syncd → timed anchor refinement
status: Draft (seed — not scheduled)
owner: @runtime
created: 2026-07-21
depends-on:
  - TASK-0297
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Wall-clock baseline: tasks/TASK-0297-time-v1-rtcd-walltime-tz-live-clock.md
  - Wall-clock contract: docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md
---

## Context (seed)

TASK-0297 anchors walltime from the goldfish RTC at boot. Long-running or
RTC-less systems need network time. `source/services/time-syncd` and
`userspace/time_sync` are placeholders today (echo stubs) — this seed
reserves the follow-up so RFC-0076 can pin NTP as an explicit non-goal.

## Scope sketch (to be turned into a full ledger when scheduled)

- SNTP (RFC 4330 subset) client in time-syncd over netstackd UDP; bounded
  parsing of server replies (`test_reject_*` for malformed/oversized).
- New vetted timed op for anchor refinement: accepted **only** from
  time-syncd's `sender_service_id`, sanity-bounded step (reject wild jumps,
  slew-not-step within a small window) — pinned in an RFC extension before
  implementation.
- Honest markers: `timesync: anchored (sntp)` only after a validated
  round-trip; no marker on stub behavior.
- Policy: outbound NTP endpoint config via settingsd key; deny-by-default
  posture for the new timed op.

## Blocking prerequisites

- netstackd UDP readiness for guest-side clients in QEMU (verify before
  scheduling).
- RFC seed extending RFC-0076 (anchor-refinement contract) — required first.
