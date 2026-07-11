<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Project Layout

The DSL uses a **file-based convention** (the Nuxt model): every `.nx` file
under `ui/` is **auto-discovered** and merged into ONE program — there are no
`import` statements. Deterministic by construction: files merge in **sorted
path order** (`compile_project_dir` / `merge_project`), never filesystem
iteration order, so the same tree always produces the same `.nxir`.

## The entry point (how an app "starts")

There is no `app.nx`; two **reserved declarations** form the entry, and they
may live in any file (conventionally `ui/pages/Routes.nx` and the home page):

- `Routes { "/" -> <Page>; … }` — the router table. The page mapped to `"/"`
  is the app's home; `navigate("<path>")` switches pages at runtime. Every
  page named here must exist somewhere under `ui/` (checker-enforced).
- `Window { style/level/mode/resizable }` — the app's window **intent**
  (docs/dev/ui/patterns/windowing/window-intent.md). Declared once, next to
  the home page by convention.

Everything else is reachable from there: a `Page`, `Store`, `Event` or
component declared in ANY `ui/**.nx` file is visible program-wide by name
(one global namespace — the checker rejects duplicates). Declaring a store
makes it live: its root `@effect`s (events nothing dispatches) fire once at
mount, and `$state.<field>` binds any page to it.

## Minimal layout (default)

- `ui/pages/**.nx` — top-level pages/screens (+ `Routes.nx` by convention)
- `ui/components/**.nx` — reusable UI components
- `ui/composables/**.nx` — **pure** helpers and store definitions (no IO)
- `ui/themes/**.nxtheme.toml` — theme authoring (human-editable)
- `ui/tests/**` — fixtures/goldens/tests (keep minimal at first)

The folder names are **convention for humans** (and for CLI generators/lints);
the compiler merges every `ui/**.nx` regardless of subfolder. The ONE
exception with semantics is `ui/platform/<profile>/` (overrides, below).

## Enterprise layout (the shipped-apps convention)

The apps in `userspace/apps/` follow the full convention — nested component
domains, one store per domain, a README per app (see `desktop-shell`,
`settings`, `search`, `chat` as living examples):

```
userspace/apps/<app>/
  README.md                     # purpose, structure, store/route overview
  manifest.toml                 # identity, bundle_type, payload_kind, caps
  i18n/en.json
  ui/
    pages/                      # routed views + Routes.nx (the entry)
    components/<domain>/        # Component declarations, nested by domain
      topbar/  dock/  tiles/    #   (desktop-shell)
      settings/                 #   (settings)
    composables/<domain>.store.nx  # ONE store per domain, never a god-store
    platform/<profile>/pages/   # Page overrides only (desktop/, phone/)
```

- **Components** carry `props:` and stay presentational; interaction handlers
  (`on Tap -> …`) attach at the USE SITE — wrap the instance in a `Stack` to
  carry the handler (handlers bind to nodes, not component instances).
- **Stores** own state + events + reducers + `@effect`s for one domain; pages
  bind via `$state.<field>` (field names are program-global — keep them
  unique across stores).
- **Tests** live in `tests/dsl_apps_conformance/` — one file per app,
  compiling the REAL project dir and driving the core interaction against a
  fake service host.

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

## Widget libraries (TASK-0081 C3)

An app's `manifest.toml` may declare `dependencies = ["<lib>"]` — sibling
folders under `userspace/apps/` with `bundle_type = "library"`. At BUILD
time (`compile_project_dir` / `nx dsl build`) every library's
`ui/components/*.nx` compiles INTO the app's one canonical `.nxir` — there
is no runtime component loading (one-program-one-hash and AOT parity stay).
Governance, fail-closed at build: a library file may declare **components
only** (compositions of system primitives — no pages/stores/events/routes,
no own modifiers or primitives); a violation or a missing dependency fails
the build with the reason.
