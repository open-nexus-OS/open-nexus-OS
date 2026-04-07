<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Chat Action Cards

Inline chat action cards are the bounded “stay in chat” action surface for low-risk flows.

Primary anchors:

- `tasks/TASK-0126D-chat-action-cards-v0-host-inline-track-confirm-open.md`
- `tasks/TRACK-SYSTEM-DELEGATION.md`

## Posture

- providers send typed data, not arbitrary UI,
- chat renders canonical card components,
- cards persist as snapshots in the transcript,
- and refresh is explicit rather than background-polled.
