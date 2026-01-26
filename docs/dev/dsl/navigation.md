<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Navigation & Routes

Navigation is designed to be deterministic:

- explicit route table,
- bounded history,
- stable param parsing and errors.

## Example (illustrative)

```nx
Routes {
  "/" -> Home
  "/detail/:id" -> Detail(id: Int)
}

// in an effect or event handler:
navigate("/detail/7")
```

Notes:

- route conflicts are lint errors
- param types must validate deterministically
