<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Virtual List

Virtual lists are required for large collections. The contract is:

- bounded work per frame,
- stable ordering and stable keys,
- deterministic layout and recycling.

## Example (illustrative)

```nx
VirtualList {
  items: $state.items
  key: (item) -> item.id
  render: (item) -> Row { Text { value: item.title } }
}
```
