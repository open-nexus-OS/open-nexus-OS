<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# CLI (`nx dsl` / `nx-dsl`)

The toolchain backend is the `nx-dsl` binary (`userspace/dsl/cli`). The `nx dsl`
shim delegates `fmt`/`lint`/`build` to it via `NX_DSL_BACKEND`; the other verbs
are invoked directly (or via `just dsl …`, which builds and runs the backend).

## Shipped verbs (v0.1)

```bash
just dsl lint ui/pages/Home.nx          # parse + check, all diagnostics
just dsl fmt --check ui/pages/Home.nx   # non-zero if formatting is needed
just dsl fmt ui/pages/Home.nx           # rewrite to the canonical layout
just dsl check app.nx                   # lint + lowering dry-run
just dsl build -o target/dsl app.nx     # emit canonical .nxir
just dsl build --emit-json app.nx       # + derived .nxir.json summary view
just dsl hash app.nx                    # print the program hash (also .nxir)
just dsl explain NX0405                 # explain a diagnostic code
just nx-dsl-shim lint app.nx            # the same through the `nx` shim
```

Exit codes: `0` ok, `1` diagnostics/violations, `2` usage/IO errors.

## Diagnostics

Every diagnostic carries a **stable code** (`NX####`) and a byte span rendered
as `file:line:col`. Codes never get renumbered; `nx-dsl explain <code>` is the
catalog. Warnings (`NX0406` profile fallback, `NX0407` unhandled result,
`NX0409` missing timeout — the latter two become errors once the async-recipe
wave lands) pass unless `--deny-warn`.

## Determinism contract

- `fmt` is a fixpoint: `fmt(parse(fmt(x))) == fmt(x)` (CI-proven);
- `build` twice ⇒ **byte-identical** `.nxir` (CI-proven); declaration order in
  the source never changes the IR;
- `.nxir.json` is a derived view for goldens/debugging — never consumed at
  runtime.

## Planned verbs (later phases)

- `run` / `snapshot` — headless interpreter + goldens (v0.1b, TASK-0076);
- `i18n extract|compile` (v0.2b, TASK-0078);
- generators, minimal-scaffold posture (v0.2b, TASK-0078):
  `init <appdir>`, `add page|component|store [--scope session|durable]|service|test <…>`;
- `session inspect|clear|export --json` — host-run debugging (v0.2b);
- `build|run|watch --aot` (v0.3a, TASK-0079).

Notes: "session state" is in-memory by default; "durable state" uses typed
snapshots via the state substrate, never an untyped file.

## Changelog

- **v0.1 (2026-07-06, TASK-0075)** — `fmt`, `lint`, `check`,
  `build [--emit-json] [-o DIR]`, `hash`, `explain`; shim delegation +
  `just dsl` / `just nx-dsl-shim` wiring.
