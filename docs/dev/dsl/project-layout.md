<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Project Layout

The DSL uses a **deterministic, explicit** layout. There is **no auto-import**.

## Minimal layout (default)

- `ui/pages/**.nx` — top-level pages/screens
- `ui/components/**.nx` — reusable UI components
- `ui/composables/**.nx` — **pure** helpers and store definitions (no IO)
- `ui/themes/**.nxtheme.toml` — theme authoring (human-editable)
- `ui/tests/**` — fixtures/goldens/tests (keep minimal at first)

Optional:

- `ui/services/**.nx` — effect-side service adapters (no reducers; v0.2b+)
- `ui/platform/<profile>/**` — deterministic profile overrides

## Naming conventions (recommended)

These are recommendations, not requirements:

- `ui/composables/**.store.nx` — store definitions (State/Event/reducers/effects)
- `ui/services/**.service.nx` — service adapters used by effects

## Tests layout (optional, generator-created)

- `ui/tests/unit/{stores,services,composables}/`
- `ui/tests/component/{pages,components}/`
- `ui/tests/e2e/`
- `ui/tests/fixtures/` + `ui/tests/goldens/`

## Philosophy: minimal by default, expand via CLI generators

We avoid heavy scaffolds with empty files. Instead, `nx dsl` creates structure **only when requested**:

- `nx dsl init`
- `nx dsl add store ...`
- `nx dsl add test ...`

See: `docs/dev/dsl/cli.md`.
