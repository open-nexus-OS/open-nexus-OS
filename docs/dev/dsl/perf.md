<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Performance

This doc captures how to measure and improve DSL performance:

- interpreter vs AOT trade-offs,
- snapshot perf gates,
- deterministic benchmarks (host-first, QEMU-gated).

At runtime, performance work should follow the retained UI pipeline contract:

- stable Scene-IR / retained-tree identity,
- deterministic text preparation and measurement,
- narrow invalidation (`paint-only`, `place-only`, `measure+place`, `text-prep+measure+place`),
- and bounded caches for large collections and responsive surfaces.

See also:

- `docs/dev/dsl/codegen.md`
- `docs/dev/dsl/incremental.md`
- `docs/dev/ui/foundations/layout/layout-pipeline.md`
