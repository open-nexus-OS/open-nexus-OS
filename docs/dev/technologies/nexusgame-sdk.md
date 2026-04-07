<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NexusGame SDK

NexusGame SDK is the platform offering for games and realtime interactive apps that need a stable path for render, input,
timing, assets, and optional social glue.

## Primary track anchor

- `tasks/TRACK-NEXUSGAME-SDK.md`

## Good fit

Use NexusGame SDK when you are building:

- small 2D or 3D games,
- UI-heavy interactive scenes,
- or pro realtime tools that share game-like timing/render/input needs.

Typical consumers:

- reference and indie games,
- UI-heavy puzzle or arcade titles,
- realtime editors and simulation-like tools,
- and apps that need render/input/timing primitives without owning the whole platform stack.

## What users experience

Users should experience:

- responsive input and pacing,
- stable rendering behavior,
- predictable multiplayer/social handoff through system delegation,
- and game features that still respect platform policy, performance, and trust rules.

## What it gives app developers

- one stable platform path for render/input/timing-heavy apps,
- cleaner composition over NexusGfx, NexusMedia, and NexusNet,
- a reusable social/delegation story instead of per-game UX plumbing,
- and deterministic test and perf hooks suitable for games and realtime tools.

## Best practice

- treat NexusGame as a composition layer over shared platform primitives,
- use NexusGfx/NexusMedia/NexusNet instead of inventing parallel stacks,
- rely on system-owned invites/chat/delegation where possible,
- and keep testability via deterministic input playback and scene goldens.

## Avoid

- embedding ad-hoc social/chat stacks inside games,
- assuming ambient access to GPU/audio/input devices,
- or treating “game SDK” as a license to bypass the normal platform authority model.

## Related docs

- `docs/dev/technologies/nexusnet-sdk.md`
- `docs/dev/technologies/zero-copy-data-plane.md`
- `docs/dev/technologies/nativewidget-runtime.md`
