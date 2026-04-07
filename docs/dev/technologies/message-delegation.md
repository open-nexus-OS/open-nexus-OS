<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Message Delegation

Message Delegation is the platform offering for apps that want to reuse system-owned communication and action flows
instead of embedding their own share, chat, contact-picker, or invite stacks.

This is the developer-facing technology view of the same platform family that the UI docs describe as System Delegation.

## Primary track anchors

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md`
- `tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md`

## Good fit

Use Message Delegation when your app wants to:

- send something through the system share/chat path,
- pick contacts or locations through a canonical system surface,
- hand work to another app or system target,
- or integrate social/messaging flows without becoming its own communications platform.

Typical consumers:

- social apps,
- games with invites/join flows,
- files/browser/media apps,
- maps and contact-aware apps,
- and any app that should delegate communication UX to the platform.

## What users experience

Users should experience:

- canonical chooser/defaults behavior,
- consistent confirmation and trust indicators,
- safer cross-app data transfer,
- and less app-specific “mini platform” duplication.

## What it gives app developers

- a canonical way to hand off user-mediated actions,
- less duplicate chooser/share/chat/contact UI to maintain,
- a clearer trust and policy story across apps,
- and stronger reuse of system-owned data-transfer and confirmation flows.

## Best practice

- pass typed data and let the system own the visible flow,
- use `content://` and grants for cross-app data,
- prefer delegation for user-mediated actions instead of embedding transport glue,
- and let SystemUI own chooser/defaults/confirmation behavior.

## Avoid

- pushing raw transport details into app UI,
- building your own privileged contact/chat/share surfaces when the platform one fits,
- or treating delegation as a way to inject arbitrary foreign UI.

## Related docs

- `docs/dev/ui/system-experiences/system-delegation/overview.md`
- `docs/dev/technologies/nexusnet-sdk.md`
