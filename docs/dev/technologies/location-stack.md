<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Location Stack

Location Stack is the platform offering for apps that need consented location features without owning GNSS, fusion, or
privacy logic themselves.

## Primary track anchor

- `tasks/TRACK-LOCATION-STACK.md`

## Good fit

Use Location Stack when your app needs:

- maps and navigation features,
- weather or local context,
- fitness or route tracking,
- or optional geotagging/location-aware media features.

Typical consumers:

- Maps and navigation apps,
- Weather and local discovery apps,
- fitness and route-recording apps,
- camera/media features with optional geotagging,
- and apps that need trusted location without becoming a sensor stack.

## What users experience

Users should experience:

- explicit location consent,
- predictable indicator behavior,
- deterministic allow/deny outcomes,
- and app features that consume trusted location data rather than raw sensor plumbing.

## What it gives app developers

- one app-facing location surface instead of direct GNSS/fusion plumbing,
- clearer privacy and policy posture by default,
- a reusable consented location model across different app categories,
- and deterministic fixture/replay-friendly behavior for host-first testing.

## Best practice

- depend on `locationd`-style app-facing location services, not device-facing GNSS protocols,
- request the smallest location scope needed,
- keep background location exceptional and explicit,
- and use fixtures/replay for host-first correctness.

## Avoid

- talking to device-facing services from app code,
- treating mock location as a normal app feature,
- or bypassing policy/privacy surfaces for “just this one app”.

## Related docs

- `docs/dev/ui/system-experiences/system-delegation/maps-pick-location.md`
