<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Maps Pick Location

`maps.pick_location` is the reusable system surface for choosing a place or location result.

Primary anchors:

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TRACK-MAPS-APP.md`

## Posture

- location picking is a bounded explicit action,
- no live tracking is implied by a pick result,
- and the returned value should stay typed and policy-safe rather than passing ad-hoc URL strings.
