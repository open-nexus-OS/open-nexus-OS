<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Runtime

The UI runtime provides:

- reactive signals/effects scheduling,
- animation and transitions hooks,
- stable frame pacing instrumentation points.

## Scheduling posture

The runtime should gather state changes into an explicit invalidation plan before touching layout work.

Recommended order:

1. apply/coalesce state commits,
2. resolve retained-tree updates,
3. classify invalidation (`paint-only`, `place-only`, `measure+place`, `text-prep+measure+place`),
4. run the cheapest valid pipeline stages,
5. hand immutable scene inputs to the renderer,
6. schedule effects after commit through explicit bounded queues.

The runtime should avoid:

- hidden relayout triggered from effect execution,
- backend-driven mutation of retained-tree state,
- and timing-based heuristics that change which pipeline stage runs.

## Ownership boundary

The runtime owns:

- retained-tree mutation,
- invalidation classification,
- focus/viewport state,
- and the authoritative ordering of resolve/text-prep/measure/place.

It should treat prepared text, line-layout artifacts, and renderer submission structures as derived data rather than
secondary sources of truth.

See also:

- `docs/dev/ui/foundations/animation.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`
