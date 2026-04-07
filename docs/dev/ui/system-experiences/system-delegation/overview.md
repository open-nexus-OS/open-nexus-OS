<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# System Delegation Overview

System delegation is the OS-native alternative to app-owned “super-app” stacks.

Primary track:

- `tasks/TRACK-SYSTEM-DELEGATION.md`

## Core contract

- apps request actions through typed intents,
- SystemUI owns chooser and confirmation surfaces,
- `policyd` remains the single allow/deny authority,
- cross-app data moves through `content://` plus scoped grants,
- and visible surfaces stay DSL-authored rather than app-injected.

## Product stance

We want “super-infrastructure”, not a “super-app”:

- chat compose,
- contacts pick,
- maps pick-location,
- social compose,
- share/open-with,
- and later bounded inline action flows.

## Related tracks

- `tasks/TRACK-NEXUSSOCIAL.md`
- `tasks/TRACK-OFFICE-SUITE.md`
- `tasks/TRACK-MAPS-APP.md`
- `tasks/TRACK-PIM-SUITE.md`
- `tasks/TRACK-NEXUSGAME-SDK.md`
- `tasks/TRACK-MAIL-APP.md`

## Related technology docs

- `docs/dev/technologies/message-delegation.md`
- `docs/dev/technologies/nexusnet-sdk.md`
