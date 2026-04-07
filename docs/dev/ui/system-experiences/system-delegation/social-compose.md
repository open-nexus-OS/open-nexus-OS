<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Social Compose

`social.compose` is the reusable system surface for composing a bounded social post through a chosen target.

Primary anchors:

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TRACK-NEXUSSOCIAL.md`

## Posture

- compose state is target-aware but UI stays canonical,
- rich attachments use grants rather than ambient file access,
- and final post/send remains explicit, user-mediated, and auditable.
