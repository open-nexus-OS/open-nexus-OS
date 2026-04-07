<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NexusNet SDK

NexusNet SDK is the main platform offering for apps that need networking, cloud access, or distributed peer features.

It gives app developers a **safer and more ergonomic** path to:

- local-first networking,
- bounded HTTP and cloud integrations,
- account-backed auth flows,
- and distributed peer features built on top of DSoftBus,

without pushing raw network/security complexity into every app.

## Primary track anchors

- `tasks/TRACK-NEXUSNET-SDK.md`
- `tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md`
- `tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md`

## Good fit

Use NexusNet SDK when your app needs:

- HTTP or REST access,
- optional cloud sync or remote services,
- account-backed sign-in,
- or nearby/distributed features such as peer discovery, pairing, or remote sharing.

Typical consumers:

- Mail, Feeds, Podcasts, and Weather,
- account-backed apps that reuse OAuth or cloud grants,
- distributed or cross-device features,
- and apps that want network features without owning raw socket/security posture.

## What users experience

When used well, users should experience:

- clear sign-in and consent moments,
- bounded, policy-aware network behavior,
- local-first UX where offline still works,
- and distributed features that feel like normal OS features rather than ad-hoc peer plumbing.

## What this includes

- bounded HTTP-style requests,
- optional cloud and sync features,
- account-backed auth integration,
- and distributed peer features through the same developer-facing platform surface.

Important posture:

- DSoftBus is part of the NexusNet story, not a separate app-developer product line.
- App developers should normally think in terms of **NexusNet capabilities**, not “embed DSoftBus directly”.

## Best practice

- prefer typed clients or `svc.*`-style surfaces over raw protocol glue,
- keep flows local-first where possible,
- use System Delegation when the flow is primarily user-mediated,
- and treat distributed features as policy- and consent-gated capabilities rather than ambient network access.

## Avoid

- building raw peer-to-peer UI and trust surfaces inside every app,
- mixing user-facing share/chat/invite UX with transport details,
- or making DSoftBus, OAuth, or HTTP stacks part of widget-level code.

## DSoftBus within NexusNet

DSoftBus is the distributed transport and discovery substrate behind the distributed side of NexusNet.

For app developers, the important rule is:

- use NexusNet when you need nearby/distributed capabilities,
- let the platform own discovery, pairing, trust, and policy posture,
- and avoid treating DSoftBus as a standalone app-facing SDK unless you are working at the platform boundary.

## Related tracks and docs

- `tasks/TRACK-NEXUSACCOUNT.md`
- `tasks/TRACK-NEXUSGAME-SDK.md`
- `tasks/TRACK-NEXUSMEDIA-SDK.md`
- `docs/distributed/dsoftbus-lite.md`
- `docs/dev/technologies/message-delegation.md`
- `docs/dev/technologies/zero-copy-data-plane.md`
