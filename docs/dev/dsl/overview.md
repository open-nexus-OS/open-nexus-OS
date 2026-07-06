<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Overview

The Nexus UI DSL (`.nx`) is a deterministic, declarative UI language for building
apps, the system shell, and the login greeter on one reactive pipeline:

```text
.nx source ──nx dsl build──▶ .nxir (canonical binary IR)
                                │
                ┌───────────────┼──────────────────┐
                ▼               ▼                  ▼
          interpreter     app-host process     AOT codegen (v0.3+)
          (host preview,  (one runtime ELF,    (per-app native binary,
           shell mount)    per-app process)     behavior-identical)
                │               │                  │
                └───────────────┴──────────────────┘
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
- **Two modes**: interpreter for fast iteration and system surfaces; optional AOT for
  maximum startup/steady-state performance — golden-proven identical.

## The shape of a program

```nx
// ui/pages/UserListPage.nx

Store UserListStore {
    users: List<User> = [],
    loading: Bool = false,
}

Event UserListEvent {
    LoadUsers,
    UsersLoaded(List<User>),
}

reduce UserListEvent {
    LoadUsers => state.loading = true,
    UsersLoaded(users) => {
        state.users = users;
        state.loading = false;
    },
}

@effect on LoadUsers {
    let users = svc.users.list();
    dispatch(UsersLoaded(users));
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
    .padding(4)
    .gap(2)
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

| Page | Contents |
|---|---|
| `principles.md` | the design principles the language enforces |
| `grammar.md` | normative EBNF for the v1 surface |
| `types.md` | the type system (scalars, composites, tokens, budgets) |
| `syntax.md` | guided syntax tour |
| `modifiers.md` | the modifier catalog (hybrid utility vocabulary) |
| `state.md` | stores, events, reducers, effects |
| `navigation.md` | routes and navigation |
| `i18n.md` | translation keys and locale catalogs |
| `profiles.md` | device environment + responsive overrides |
| `services.md` | `svc.*` adapters and the effect boundary |
| `db-queries.md` | QuerySpec: typed, bounded, deterministic data access |
| `ir.md` | the canonical IR (`.nxir`): schema, identity, evolution |
| `codegen.md` | AOT codegen contract |
| `incremental.md` | incremental builds |
| `runtime.md` | app lifecycle, app-host, surfaces, cold start |
| `perf.md` | performance budgets and gates |
| `testing.md` | host tests, snapshots, conformance corpus |
| `cli.md` | the `nx dsl` command surface |
| `project-layout.md` | app project structure |
| `patterns.md` | component/props patterns (instead of generics) |
