<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Safe Ads And Family Mode

Safe Ads and Family Mode is the platform offering for apps that need monetization or outbound navigation while staying
inside a child-safe, policy-gated UX model.

## Primary track anchor

- `tasks/TRACK-ADS-SAFETY-FAMILYMODE.md`

## Good fit

Use this technology when your app:

- is ad-supported,
- wants safe click-out or install flows,
- targets families or children,
- or needs a clear “what happens under Family Mode?” story.

Typical consumers:

- casual and kids’ games,
- free apps with ads or promotion flows,
- Store/Browser-adjacent click-out paths,
- and apps that need a standardized monetization posture instead of arbitrary web ad runtimes.

## What users experience

Users should experience:

- clear and predictable ad surfaces,
- no deceptive close behavior,
- explicit confirmation before leaving the app,
- and stronger default protections when Family Mode is active.

## What it gives app developers

- one safer monetization and click-out posture to build against,
- clearer rules for family-safe behavior,
- less need for app-specific ad/navigation policy guesswork,
- and a path to monetization that fits the platform’s trust model.

## Best practice

- treat ads as a standardized platform surface, not arbitrary app-embedded JS,
- keep navigation to Store/Browser user-mediated,
- assume Family Mode and policy gates are part of the default integration story,
- and clearly separate in-app content from “leave this app” actions.

## Avoid

- arbitrary HTML/JS ad runtimes in app UI,
- store bounce / browser bounce patterns,
- or dark-pattern monetization that depends on hidden or moving controls.

## Related docs

- `docs/dev/technologies/message-delegation.md`
- `docs/dev/ui/system-experiences/system-delegation/confirmation-and-consent.md`
