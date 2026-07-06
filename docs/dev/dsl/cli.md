<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# CLI (`nx dsl` / `nx-dsl`)

The toolchain backend is the `nx-dsl` binary (`userspace/dsl/cli`). The `nx dsl`
shim delegates `fmt`/`lint`/`build` to it via `NX_DSL_BACKEND`; the other verbs
are invoked directly (or via `just dsl …`, which builds and runs the backend).

## Shipped verbs

```bash
just dsl lint ui/pages/Home.nx          # parse + check, all diagnostics
just dsl fmt --check ui/pages/Home.nx   # non-zero if formatting is needed
just dsl fmt ui/pages/Home.nx           # rewrite to the canonical layout
just dsl check app.nx                   # lint + lowering dry-run
just dsl build -o target/dsl app.nx     # emit canonical .nxir (file OR appdir)
just dsl build --emit-json app.nx       # + derived .nxir.json summary view
just dsl hash app.nx                    # print the program hash (also .nxir)
just dsl explain NX0405                 # explain a diagnostic code
just nx-dsl-shim lint app.nx            # the same through the `nx` shim
```

### `run` — headless execution

```bash
nx-dsl run examples/dsl/masterdetail                # mount, print scene texts
nx-dsl run <app> --route /detail --profile phone    # navigate + device profile
nx-dsl run <app> --locale de                        # i18n/<locale>.json|.nxc chain
nx-dsl run <app> --transcript t.txt --dispatch LoadRequested
```

Mounts the program (a `.nx` file or an app directory), applies the profile's
device environment (`desktop`/`phone`/`tablet`/`tv` fixtures), optionally
navigates, optionally dispatches ONE event with effects replayed from a
transcript (see [services.md](services.md)), then prints a deterministic
summary: program hash + pre-order scene texts. Any transcript replay miss =
exit 1.

### `i18n` — catalogs

```bash
nx-dsl i18n extract examples/dsl/masterdetail -o i18n/en.json  # program keys → authoring JSON
nx-dsl i18n compile i18n/en.json -o i18n/en.nxc                # JSON → binary catalog
```

`extract` reads the key table from the LOWERED IR (it can never drift from
execution) and preserves translations already present in the output file.
`compile` emits the deterministic `NXC1` binary the runtime loads via
`Catalog::from_binary`; empty (untranslated) entries are omitted so the
locale chain falls through to the next catalog / pseudo-locale instead of
rendering `""`.

### Generators

```bash
nx-dsl init myapp                # minimal buildable skeleton (store/page/routes)
nx-dsl add page Settings myapp   # one canonical file each
nx-dsl add component Chip myapp
nx-dsl add store Session myapp
```

Generated sources are canonical-format `.nx`; `init → build` green is pinned
by a scaffold test. Generators refuse to overwrite existing files.

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

- `snapshot` — pixel goldens from the CLI (the harness lives in
  `tests/dsl_goldens` today);
- `add service|test`, `session inspect|clear|export --json` — host-run
  debugging (v0.2b remainder);
- `build|run|watch --aot` (v0.3a, TASK-0079).

Notes: "session state" is in-memory by default; "durable state" uses typed
snapshots via the state substrate, never an untyped file.

## Changelog

- **v0.2b (2026-07-06, TASK-0078)** — `run` gains
  `--route/--locale/--profile/--transcript/--dispatch` (headless runs against
  transcripts); `i18n extract|compile` (authoring JSON ↔ `NXC1` binary
  catalogs); generators `init` + `add page|component|store`; project-dir
  inputs everywhere `build` accepts them.
- **v0.1 (2026-07-06, TASK-0075)** — `fmt`, `lint`, `check`,
  `build [--emit-json] [-o DIR]`, `hash`, `explain`; shim delegation +
  `just dsl` / `just nx-dsl-shim` wiring.
