<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Zero-Copy Data Plane

Zero-Copy Data Plane is the platform offering for apps and runtimes that need to move large payloads efficiently without
turning every handoff into repeated copy work.

It is the preferred path when bulk bytes matter more than control-plane convenience.

## Primary task anchors

- `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md`
- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md`
- `tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md`

## Good fit

Use Zero-Copy Data Plane when your feature has:

- large media payloads,
- image, document, or attachment transfer,
- rich previews or binary blobs,
- heavy render or media pipelines,
- or any repeated handoff where copies would dominate latency, memory pressure, or energy.

Typical consumers:

- Files and document flows,
- share/send surfaces,
- image/video/media pipelines,
- NativeWidget-heavy surfaces,
- and NexusNet distributed transfers with meaningful payload size.

## What users experience

When used well, users should experience:

- faster large-file and media handling,
- fewer stalls from repeated serialization/copy steps,
- better battery and memory behavior on data-heavy flows,
- and UI that stays responsive while binary payloads move in the background.

## What it gives app developers

- one canonical bulk-data path instead of ad-hoc byte copying between services,
- clearer separation between typed control messages and bulk payload movement,
- a more honest performance story for previews, attachments, and render/media pipelines,
- and a stable base for later optimization without changing the app-facing shape.

## Best practice

- keep the control plane small, typed, and auditable,
- move bulk payloads onto explicit handles or buffers,
- reserve zero-copy for payload-heavy paths where it materially matters,
- and measure copy-fallback, mapping, and reuse behavior honestly.

## Avoid

- marketing every shared buffer as “zero copy” without proving a real mapped/consumed shared-object path,
- mixing small command/control messages into bulk handles,
- or building bespoke per-app transfer tricks when the common platform path fits.

## Related surfaces

- Files and document access flows,
- share and send surfaces,
- NexusNet distributed/bulk-transfer paths,
- NativeWidget- and media-heavy surfaces.

## Related docs

- `docs/dev/technologies/nexusnet-sdk.md`
- `docs/dev/technologies/nativewidget-runtime.md`
- `docs/dev/ui/system-experiences/document-access/README.md`
