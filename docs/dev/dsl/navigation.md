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

## Shipped (v0.2a, TASK-0077)

- `Routes { "/" -> Home; "/detail/:id" -> Detail(id: Int); }` — typed params:
  an `Int` param rejects non-numeric text (the route simply doesn't match).
- **`navigate` handler action** (IR v1.1): `on Tap -> navigate("/detail/7")` —
  the path is a `Str` expression evaluated at emit time; a literal that
  matches no declared route is a compile diagnostic, a dynamic miss is a
  deterministic runtime error.
- Bounded history (32 entries), `push`/`replace`/`back`; the root entry never
  pops. Route changes re-emit with layout damage.
- Open (this task's remainder): route params bound into page views; kept-alive
  route state contract.
