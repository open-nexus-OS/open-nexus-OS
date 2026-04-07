<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Chat Compose

`chat.compose` is the reusable system surface for sending a message through the default or chosen chat target.

Primary anchors:

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TRACK-NEXUSSOCIAL.md`

## Posture

- caller provides typed data only,
- the receiving chat surface renders canonical UI,
- identity and transport labels must be safe and non-spoofable,
- and the send path remains policy-gated and auditable.
