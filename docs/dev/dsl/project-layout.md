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

## Target app layout (TASK-0081, decided 2026-07-07)

A FULL app in `userspace/apps/<name>/` carries everything one app is, in one
folder (the `bundles/` split is being consolidated away):

```
userspace/apps/<name>/
  manifest.toml            # identity, caps, payload_kind, exports, dependencies
  ui/pages/**.nx           # the ui/ layout above, unchanged
  ui/components/**.nx
  ui/composables/**.store.nx
  ui/services/**.service.nx
  ui/platform/<profile>/**
  i18n/<locale>.json       # authored catalogs (nx-dsl i18n extract/compile)
  assets/**                # images/icons/sounds → manifest `resources`,
                           # referenced from DSL via the Image/Icon primitives
                           # (IR AssetRef; wiring lands with TASK-0081)
  native/ (optional)       # companion Rust crate: the tooling turns it into
                           # its OWN process with its OWN manifest caps; the
                           # DSL app calls it through generated svc.* signatures
```

Rules that keep this from becoming a wild west (see TASK-0081 for the full
contract): `native/` may only link the curated SDK crate set; component
libraries resolve at BUILD time into the single canonical `.nxir`; every
capability — system or app-defined — is declared in the manifest and enforced
fail-closed.
