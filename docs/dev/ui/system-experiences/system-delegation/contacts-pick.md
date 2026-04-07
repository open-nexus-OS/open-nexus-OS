<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contacts Pick

`contacts.pick` is the reusable system surface for selecting one or more contact objects.

Primary anchors:

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TRACK-PIM-SUITE.md`

## Posture

- contacts are returned as typed objects or `content://` references,
- grants stay explicit and scoped,
- and apps should not re-implement their own privileged contact-browser UX when the shared surface is sufficient.
