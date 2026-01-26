<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Overview

The Nexus UI DSL is a deterministic, host-testable UI language that targets the OS UI stack (runtime/layout/kit/theme).

Key properties:

- **Deterministic**: stable outputs for stable inputs (formatting, lowering, IR, goldens).
- **No auto-import**: module resolution is explicit and reproducible.
- **Two modes**: interpreter (fast iteration) and optional AOT codegen (startup/perf).

## 10-minute tour

### Project layout (minimal)

See `docs/dev/dsl/project-layout.md`.

### Example (illustrative)

```nx
// ui/pages/Home.nx
Page Home {
  view: Stack {
    Text { value: @t("home.title") }
    Button { label: @t("home.cta"); on Tap -> emit(HomeEvent::Clicked) }
  }
}
```

```nx
// ui/composables/home.store.nx
Store HomeStore {
  State { clicks: Int }
  Event { Clicked }

  reduce(state, event) -> state {
    match event {
      Clicked => state.clicks += 1
    }
  }

  // Effects run after commit and may call services (timeouts/bounds required).
  effect(event) {
    // v0.2b: svc.* calls go here (not in reducers).
  }
}
```

## Next

- Project structure: `docs/dev/dsl/project-layout.md`
- CLI: `docs/dev/dsl/cli.md`
- State/effects: `docs/dev/dsl/state.md`
- Patterns (generics-like ergonomics): `docs/dev/dsl/patterns.md`
- Testing: `docs/dev/dsl/testing.md`
