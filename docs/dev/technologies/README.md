<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Technologies

This section documents the platform as a set of **developer-facing capabilities and products**.

Use it when you want to answer questions like:

- what the platform offers me as an app developer,
- which technology is the right fit for my app or feature,
- which user-facing flows and policies come with it,
- and what best practice looks like at the integration layer.

Do not use this section for:

- daemon-internal implementation walkthroughs,
- protocol bring-up notes,
- or kernel/service architecture details that belong in ADRs, RFCs, or lower-level subsystem docs.

Current entry points:

- `docs/dev/technologies/dev-mode.md`
- `docs/dev/technologies/family-mode.md`
- `docs/dev/technologies/managed-devices.md`
- `docs/dev/technologies/nexusnet-sdk.md`
- `docs/dev/technologies/nexus-account.md`
- `docs/dev/technologies/message-delegation.md`
- `docs/dev/technologies/queryspec.md`
- `docs/dev/technologies/location-stack.md`
- `docs/dev/technologies/nexusgame-sdk.md`
- `docs/dev/technologies/safe-ads-and-family-mode.md`
- `docs/dev/technologies/zero-copy-data-plane.md`
- `docs/dev/technologies/nativewidget-runtime.md`

Suggested mental model:

- **Developer machine posture**: `dev-mode.md`
- **Household and managed device posture**: `family-mode.md`, `managed-devices.md`
- **Identity & trust**: `nexus-account.md`, `safe-ads-and-family-mode.md`
- **Networking & delegated communication**: `nexusnet-sdk.md`, `message-delegation.md`
- **Data and app surfaces**: `queryspec.md`, `zero-copy-data-plane.md`
- **Context and sensors**: `location-stack.md`
- **Interactive/media runtimes**: `nexusgame-sdk.md`, `nativewidget-runtime.md`

Rule of thumb:

- if the question is “which platform offering should my app build on, and what is the recommended way to use it?”,
  start here.
