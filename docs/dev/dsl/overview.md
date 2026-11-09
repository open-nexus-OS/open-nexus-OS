<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Overview

The Nexus UI DSL (`.nx`) is a deterministic, declarative UI language for building
apps, the system shell, and the login greeter on one reactive pipeline:

```text
.nx source ──nx dsl build──▶ .nxir (canonical binary IR)
                                │
                ┌───────────────┴──────────────────┐
                ▼                                  ▼
          interpreter (host)              app-host process (OS)
          (preview, goldens,              (one runtime ELF, one process
           conformance corpus)             per app — the shipped tier)
                │                                  │
                └───────────────┬──────────────────┘
                                ▼
             LayoutNode → LayoutEngine → retained scene → gfx
```

Key properties:

- **Deterministic**: same source ⇒ byte-identical IR; same IR + same inputs ⇒ identical
  frames, on every tier. No floats, no wall-clock, no unordered iteration in semantics.
- **Bounded**: no unbounded loops or recursion — reducers/effects are total by
  construction; all collections and strings carry caps.
- **Encapsulated**: components expose only props + events; stores mutate only through
  reducers; IO exists only in effects via typed `svc.*` adapters
  (see `principles.md` — the theory, enforced so users never have to know it).
- **Host-first testable**: parse/lint/build/run/snapshot all work on the host; the same
  checker core is `no_std`-capable and later runs in-system.
- **One execution tier**: the same interpreter runs on the host (preview, goldens,
  conformance corpus) and inside the app-host process on the OS. An ahead-of-time
  codegen tier was retired by decision on 2026-09-09 (TASK-0079): the measured cost
  centres are layout/paint/present and the never-freeing emit heap, not expression
  interpretation — see `perf.md`.

## The shape of a program

```nx
// ui/pages/UserListPage.nx

Store UserListStore {
    users: List<User> = [],
    loading: Bool = false,
    error: Int = 0,
}

Event UserListEvent {
    LoadUsers,
    UsersLoaded(List<User>),
    LoadFailed(Int),
}

reduce UserListEvent {
    LoadUsers => state.loading = true,
    UsersLoaded(users) => {
        state.users = users;
        state.loading = false;
    },
    LoadFailed(code) => {
        state.error = code;
        state.loading = false;
    },
}

@effect on LoadUsers {
    match svc.users.list() {
        Ok(users) => dispatch(UsersLoaded(users)),
        Err(e) => dispatch(LoadFailed(e)),
    }
}

Page UserListPage {
    Stack {
        if $state.loading {
            Text(@t("common.loading"))
        } else {
            List($state.users) { user in
                Text(user.name).key(user.id)
            }
        }
    }
    .padding(16)
    .gap(8)
}
```

Reading order: **Store** (what do we keep?) → **Event** (what can happen?) →
**reduce** (pure state updates) → **@effect** (side effects via services) →
**Page** (declarative view). That is the whole model — there is no second way to hold
state, perform IO, or build UI.

## Responsive by default

Every program runs against a read-only `device.*` environment (profile, size class,
input capabilities) resolved from the platform's product/profile/shell manifests. A
page is written once for the default; device variants come from `if device.profile == …`
branches or per-profile file overrides (`ui/platform/<profile>/…`). See `profiles.md`.

## Document map

**Learn the language** — read in this order:

| Page | Contents |
|---|---|
| [syntax.md](syntax.md) | a guided tour: declarations, views, components, conditionals, loops, motion, layers |
| [state.md](state.md) | stores, events, reducers, effects — how data flows |
| [navigation.md](navigation.md) | routes and navigation |
| [profiles.md](profiles.md) | the device environment and responsive overrides |
| [i18n.md](i18n.md) | translation keys and locale catalogs |
| [services.md](services.md) | `svc.*` calls and the effect boundary |
| [patterns.md](patterns.md) | composition: components, props, slots |
| [testing.md](testing.md) | host tests, snapshots, the conformance corpus |

**Reference** — look things up:

| Page | Contents |
|---|---|
| [widgets.md](widgets.md) | every widget: props, binds, modifiers, accessibility, an example |
| [modifiers.md](modifiers.md) | every modifier: arguments, field class, the control props |
| [window.md](window.md) | the `Window { … }` declaration: frame, mode, level, size |
| [types.md](types.md) | the type system (scalars, composites, tokens, budgets) |
| [grammar.md](grammar.md) | the normative grammar (EBNF) |
| [db-queries.md](db-queries.md) | QuerySpec: typed, bounded, deterministic data access |
| [cli.md](cli.md) | `nx-dsl`: lint, fmt, build, run, render, i18n |

**How it works** — the machinery behind the language:

| Page | Contents |
|---|---|
| [principles.md](principles.md) | the design principles the language enforces |
| [ir.md](ir.md) | the canonical IR (`.nxir`): schema, identity, evolution |
| [runtime.md](runtime.md) | app lifecycle, app-host, surfaces, cold start |
| [perf.md](perf.md) | performance budgets and gates |
| [project-layout.md](project-layout.md) | an app project's structure |

Every example on these pages compiles, and every widget and modifier of the compiler's catalog
has its reference entry: `tests/dsl_conformance/tests/docs_examples.rs` checks both.
