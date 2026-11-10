<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Project Layout

A DSL app is a **directory convention**: every `.nx` file under `ui/` is discovered and
merged into ONE program — no file lists, no manifest of sources. Files merge in **sorted
path order** (`merge_project` in `userspace/dsl/core/src/project.rs`), never filesystem
iteration order, so the same tree always produces the same `.nxir`
([ir.md](ir.md)).

A file may begin with `import "…"` lines; the parser and formatter accept them, but nothing
resolves them. Declarations are visible program-wide by name, so an import changes nothing —
do not write one.

## The entry point

There is no `app.nx`. Two **reserved declarations** form the entry and may live in any file
(by convention `ui/pages/Routes.nx` and the home page):

- `Routes { "/" -> <Page>; … }` — the router table. The page mapped to `"/"` is the home
  page (if there is none, the first page in name order); `navigate("<path>")` switches pages
  at runtime. Every page named must exist (the checker rejects the rest); a path declared
  twice is `NX0408`. See [navigation.md](navigation.md).
- `Window { … }` — the app's window **intent**, declared at most once, next to the home page
  by convention. See [window.md](window.md).

Everything else is reachable from there: a `Page`, `Store`, `Event` or `Component`
declared in ANY `ui/**.nx` file is visible program-wide (one global namespace — the checker
rejects duplicates, `NX0202`). Declaring a store makes it live: its root `@effect`s (events
nothing dispatches) fire once at mount, and `$state.<field>` binds any page to it. Store
field names are program-global, so keep them unique across stores.

## Layout

```text
<app>/
  manifest.toml               # identity, bundle type, payload kind, capabilities, dependencies
  i18n/en.json                # authored catalogs: <locale>.json
  assets/                     # artwork the manifest names (icon_svg = "assets/icon.svg")
  native/                     # optional companion crate (below)
  ui/
    pages/                    # Page declarations, plus Routes.nx
    components/<domain>/      # Component declarations, nested by domain
    composables/<domain>.store.nx   # one store per domain
    platform/<profile>/pages/ # Page overrides only
```

The folder names under `ui/` are **convention for humans and for the generators**
([cli.md](cli.md)): the compiler merges every `ui/**.nx` regardless of subfolder. The one
exception with semantics is `ui/platform/<profile>/`.

- `nx-dsl init <dir>` creates `ui/composables/app.store.nx`, `ui/pages/MainPage.nx` and
  `ui/pages/Routes.nx`; it builds out of the box.
- `nx-dsl add page|component|store <Name> [dir]` creates one canonical file each;
  a store goes to `ui/composables/<name>.store.nx`.
- The shipped apps in `userspace/apps/` (`desktop-shell`, `settings`, `chat`, `stash`,
  `calculator`, `greeter`, `ime-ui`) and the sample in `examples/dsl/masterdetail` follow
  this layout, and the structure gate below holds them to it.

### Platform overrides

`ui/platform/<profile>/**.nx` may contain `Page` declarations only, and each must override a
page that exists. An override does not replace the base page in the IR — it wraps it:

```text
ui/platform/phone/pages/Home.nx   ⇒   Page Home {
ui/pages/Home.nx                          if device.profile == phone { <override view> }
                                          else { <base view> }
                                      }
```

One canonical `.nxir` therefore serves every profile, and the runtime re-selects the arm
when the profile changes at runtime ([runtime.md](runtime.md), [profiles.md](profiles.md)).
Arms are ordered by profile name.

### Components, stores, handlers

- **Components** carry `props:` and stay presentational. An interaction handler
  (`on Tap -> …`) attaches at the USE SITE: handlers bind to nodes, not component instances,
  so wrap the instance in a `Stack` that carries the handler. Give a flex-grown wrapper in a
  row `.direction(row)` so its subtree fills the allocation, and remember that
  `.overlay()` children are visited last by the layout engine ([patterns.md](patterns.md),
  [modifiers.md](modifiers.md)).
- **Stores** own state, events, reducers and `@effect`s for one domain; pages bind via
  `$state.<field>` ([state.md](state.md)). A component's own `state:` block is per instance.
- **Tests** for the real apps live in `tests/dsl_apps_conformance/`, one file per app: it
  compiles the real project directory and drives the core interaction against a fake service
  host ([testing.md](testing.md)).

## The structure gate

Every app in `userspace/apps/` is held to the layout by
`tests/dsl_apps_conformance/tests/app_structure.rs`, so a shipped app reads like a template a
new app can copy:

| Rule | What the gate requires |
|---|---|
| Thin pages | a page composes feature components and binds stores — at most 100 logical lines (neither blank nor a `//` comment) |
| Small parts | a component at most 150 logical lines, a store at most 200; split by feature or by domain beyond that |
| Feature folders | an app's components live in `ui/components/<feature>/`, never flat; a library (no pages) stays flat, as the resolver reads one level |
| Routes | `ui/pages/Routes.nx` once an app has more than one page |
| Copy | `i18n/de.json` (German first); every visible string goes through `@t()` |
| README | a `README.md` per app with the template's `## ` sections — an app: Routes, Stores and events, Services, Window, Structure, Tests (and Known limits); a library: Components, Tests. [`userspace/apps/calculator/README.md`](../../../userspace/apps/calculator/README.md) and [`userspace/apps/window-kit/README.md`](../../../userspace/apps/window-kit/README.md) are the templates |
| Tests | a conformance test in `tests/dsl_apps_conformance/tests/` compiles the app and drives it |
| One source | no `.nx` file is a byte copy of another app's — shared UI belongs in a library |

What an app does not meet yet is listed in `tests/dsl_apps_conformance/app_structure.baseline`
(`<rule> <path> [<recorded value>]`). The list only shrinks: a new violation fails, a file that
grows past its recorded size fails, and an entry that no longer applies fails until it is
deleted. Fix the app, then delete its line.

Beside the layout, every app is driven once over by
`tests/dsl_apps_conformance/tests/app_smoke.rs`: on a phone and on the desktop it mounts every
route and dispatches every event case with a neutral payload (each declared type's zero, a
record field by field — `Runtime::case_probes`, `Runtime::route_probes`) while every service
refuses, so each effect takes its failure arm. A reducer that cannot take an empty string, a
zero or an empty list, or an expression that rejects an omitted prop, fails there rather than
on the device. A case whose payload is a bare service result type (opaque until service
signatures land) has no neutral value; that app's own test drives it.

**Splitting a page keeps the scene.** Components are inlined at compile time, so moving a
subtree into a component with the same nesting emits the same scene: same boxes, same handler
payloads. Two things do change it — a component reference inside a `List` template needs a
keyed wrapper `Stack` (a collection's key is read from a widget root). A component that renders
its slots splits like any other: a slot body may place the enclosing component's own slot
(`Card { } { body { Slot body } }`), forwarding what it received into the part (RFC-0084
amendment) — `WinPane` composes `WinPaneHead`, `WinPaneBody` and `WinPaneFoot` this way.

## The manifest

`manifest.toml` is read by the bundle tooling (`tools/nxb-pack`), not by the DSL compiler,
except for `dependencies`. The keys the shipped apps use:

| Key | Meaning |
|---|---|
| `name`, `version`, `min_sdk` | identity |
| `bundle_type` | `app`, `settings`, … or `library` |
| `payload_kind` | `ui-program` for a DSL app (what execd hands to app-host); `library` for a library |
| `abilities`, `caps` | launchable entry points and the capabilities the app is granted — deny by default |
| `icon`, `icon_svg` | launcher glyph and artwork |
| `dependencies` | sibling library folders compiled into this app (below) |
| `restorable = true` | the app's `@persist` fields are snapshotted and restored ([runtime.md](runtime.md)) |

The compiler also reads `i18n/en.json` (the default locale, baked into the program so `@t()`
renders real text; a malformed catalog fails the build) and `native/surface.toml`
(below). Other locales are catalogs the bundle ships as locale packs ([i18n.md](i18n.md)).

## Widget libraries

An app lists `dependencies = ["<lib>"]` — sibling folders under `userspace/apps/` that have
a `manifest.toml` (the shipped one is `window-kit`, `bundle_type = "library"`). At BUILD
time (`nx-dsl build`, `compile_project_dir`) the library's `ui/components/*.nx` are compiled
INTO the app's one canonical `.nxir`: there is no runtime component loading, one program has
one hash. Rules, all fail-closed at build:

- a file in `ui/components/` declares **components only**; a file in `ui/composables/` declares
  **stores, events, reducers and effects only** (the state its components show) — no pages or
  routes anywhere;
- only the components the app references, transitively through the library's own
  components, are pulled in — and a composable when the app or a component it pulled in reads
  a field of its store (`$state.<field>`): the window kit's chrome store comes with the chrome,
  and an app reacts to a kit event with an effect of its own
  (`@effect on WinMenuPick(id) { dispatch(NavPick(id)); }`), never by declaring it again;
- a missing dependency folder, an empty `ui/components`, or a dependency the app never
  references fails the build with the reason.

## Native companion

`nx-dsl add native <appdir>` scaffolds `<appdir>/native/` — `surface.toml`, a `Cargo.toml`
and `src/lib.rs`. `surface.toml` declares the companion service's methods
(`[[method]]` with `name`, `args`, `result`); on every project build the checker accepts
each as `svc.<app>.<name>(…)` for that app, with the declared arity. A malformed
`surface.toml` fails the build rather than shrinking the checker's surface
([services.md](services.md)).
