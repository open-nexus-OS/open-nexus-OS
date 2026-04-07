<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NexusAccount

NexusAccount is the platform’s optional online account offering for apps that need account reuse, cloud storage, sync, or
backup upload flows.

It is meant to feel easy for users and predictable for developers, while still preserving:

- local login staying local,
- per-app grants,
- bounded OAuth/cloud behavior,
- and clear separation between account management and app usage.

## Primary track anchor

- `tasks/TRACK-NEXUSACCOUNT.md`

## Good fit

Use NexusAccount when your app needs:

- a reusable signed-in identity,
- cloud-backed storage or sync,
- backup upload/download integration,
- or future paid/entitlement-style account reuse.

Typical consumers:

- Mail and PIM-style apps,
- social/media apps,
- backup and cloud storage flows,
- and apps that should reuse a trusted account model instead of inventing their own auth stack.

## What users experience

Users should experience:

- one place to add/manage accounts,
- explicit per-app access grants,
- optional cloud features instead of cloud lock-in,
- and clear “this works without NexusAccount too” posture where appropriate.

## What it gives app developers

- a reusable account and grant model instead of per-app auth reinvention,
- a cleaner path to cloud storage, sync, and account-backed features,
- stronger security defaults around token handling,
- and a user story that separates account management from ordinary app UI.

## Best practice

- treat NexusAccount as an optional platform service, not a forced login wall,
- request the smallest per-app scope set possible,
- keep account management in system-owned surfaces,
- and let secure token handling stay behind platform services.

## Avoid

- assuming every user has or wants NexusAccount,
- exposing refresh tokens to app code,
- or turning app UI into a duplicate account-management system.

## Related docs

- `docs/dev/technologies/nexusnet-sdk.md`
- `docs/dev/ui/patterns/identity-and-trust/README.md`
