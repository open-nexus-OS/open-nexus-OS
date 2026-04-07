<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Virtual List

Virtual lists are required for large collections. The contract is:

- bounded work per frame,
- stable ordering and stable keys,
- deterministic layout and recycling.

Recommended implementation posture:

- keep a stable scroll anchor by item key,
- separate row/cell measurement from placement,
- use bounded height/cell caches for mixed-size collections,
- and remeasure only rows affected by content or width-bucket changes.

## Anchor contract

The default scroll anchor should be:

- leading visible item key,
- plus a deterministic intra-item offset.

Required behavior:

- append must not move the anchor when the viewport is not following the tail,
- prepend must preserve the same logical content under the viewport,
- filtering/reordering must either preserve the anchor or fall back deterministically to the nearest surviving key.

## Placeholder and mixed-height posture

For lazy/partial data:

- unseen items may use estimated heights,
- placeholders must use deterministic template heights keyed by width bucket,
- visible rows replace estimates with measured heights when real content arrives,
- and the correction path must adjust placement without causing a full list relayout.

## Bounded knobs

Implementations should document and test explicit bounds for:

- overscan,
- recycle pool size,
- cached row/cell count,
- estimated-height correction window,
- and max per-frame mount/recycle work.

## Example (illustrative)

```nx
VirtualList {
  items: $state.items
  key: (item) -> item.id
  render: (item) -> Row { Text { value: item.title } }
}
```

See also:

- `docs/dev/ui/foundations/layout/layout-pipeline.md`
- `docs/dev/ui/collections/lazy-loading.md`
