<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# CLI (`nx-dsl`)

The toolchain is the `nx-dsl` binary (`userspace/dsl/cli`). `just dsl <verb> …` builds it
and runs it; the `nx dsl` shim delegates only `fmt`, `lint` and `build` to it.

```bash
just dsl lint ui/pages/Home.nx          # same as: target/debug/nx-dsl lint …
just nx-dsl-shim lint app.nx            # through the shim: nx dsl lint -- app.nx
```

The shim runs the binary named by `NX_DSL_BACKEND` (the `just` recipe sets it); without
it, `nx dsl` reports the backend as unsupported.

## Verbs

| Verb | Input | What it does |
|---|---|---|
| `fmt [--check] <file…>` | `.nx` files | rewrite to the canonical layout; with `--check`, exit 1 and name the files that would change |
| `lint [--deny-warn] <file…>` | `.nx` files | parse and check; report every diagnostic |
| `check [--deny-warn] <file…>` | `.nx` files | `lint`, then a lowering dry run |
| `build [-o DIR] [--emit-json] <input>` | `.nx` file or app directory | emit `<stem>.nxir` into `DIR` (default `target/dsl`); prints path, byte count and program hash |
| `hash <input>` | `.nx` file or `.nxir` | print the program hash |
| `run <input> [flags]` | `.nx` file or app directory | mount headless and print a summary (below) |
| `render -o OUT.png <input> [flags]` | `.nx` file or app directory | a host preview as PNG (below) |
| `explain <NXcode>` | a code | one-line meaning of a diagnostic; exit 1 for an unknown code |
| `i18n extract` / `i18n compile` | app directory / JSON catalog | locale catalogs (below) |
| `init <dir>` · `add …` | | generators (below) |

An *app directory* is a project laid out as in [project-layout.md](project-layout.md):
`ui/**.nx` plus the library components the manifest's `dependencies` reference are merged
into one program. `fmt`, `lint`, `check` and `hash` of a `.nx` source read single files
only. Arguments that start with `-` are never taken as input paths.

```bash
just dsl fmt --check ui/pages/Home.nx   # non-zero if formatting is needed
just dsl build -o target/dsl app.nx     # file OR app directory
just dsl build --emit-json app.nx       # + the derived .nxir.json summary (see ir.md)
just dsl hash app.nx
just dsl explain NX0405
```

Exit codes: `0` ok · `1` diagnostics or violations · `2` usage or IO errors (a missing
file, an unknown profile, a bad flag).

### `run` — headless execution

```bash
nx-dsl run examples/dsl/masterdetail                # mount, print scene texts
nx-dsl run <app> --route /detail --profile phone    # navigate + device profile
nx-dsl run <app> --locale de                        # app locale catalog
nx-dsl run <app> --transcript t.txt --dispatch LoadRequested
```

| Flag | Meaning |
|---|---|
| `--profile P` | device fixture: `desktop` (default), `phone`, `tablet`, `tv`; anything else exits 2 |
| `--route PATH` | navigate after mounting |
| `--locale L` | read `<app>/i18n/L.nxc` (preferred) or `L.json`, chained onto the identity locale (a key renders as its own text); no readable catalog exits 1 |
| `--dispatch CASE` | dispatch ONE event, named by its case (looked up across the program's events, no arguments) |
| `--transcript FILE` | replay recorded service results for that dispatch ([services.md](services.md)); a replay miss or malformed line exits 1 |

Output: `mounted: hash <h>, profile <p>, <n> deps`, then one `text: …` line per text node of
the scene, pre-order. `run` does not execute the program's root `@effect`s; `render` does.

### `render` — a host preview as PNG

```bash
nx-dsl render -o x.png userspace/apps/calculator                  # light, 1280x800
nx-dsl render -o x.png --theme dark --size 340x600 --surface window <app>
nx-dsl render -o x.png --solid <app>                              # reduce transparency
nx-dsl render -o x.png --dispatch SetPanel:control <shell>        # open a state first
```

| Flag | Meaning |
|---|---|
| `-o OUT.png` | required |
| `--theme light\|dark` | default `light` |
| `--size WxH` | default `1280x800`, clamped to 16–7680 × 16–4320 |
| `--solid` | the reduce-transparency theme: no blur, glass roles solid |
| `--surface desktop\|window\|fullscreen` | the page base; default `desktop` when the path contains `desktop-shell` or `greeter`, else `window` |
| `--profile P`, `--route PATH` | as for `run` |
| `--locale L` | as for `run`; **default `de` when the app ships a German catalog**, else the identity locale |
| `--dispatch CASE[:arg,…]` | repeatable, applied in order after the load effects; arguments are `true`/`false`, integers, decimals or strings |

It compiles and mounts like `run`, runs the load effects against no services (a screen shows
its empty or failure state), applies `--route` and each `--dispatch`, lays out at `--size`,
paints boxes with the shared painter and text runs with the baked atlas the way app-host
rasters a surface, and composites over a wallpaper stand-in. The compositor's blur and
motion are not drawn and scroll layers draw at rest. A development aid for design work: the
QEMU shots stay the proof.

### `i18n` — catalogs

```bash
nx-dsl i18n extract examples/dsl/masterdetail -o i18n/en.json  # program keys → authoring JSON
nx-dsl i18n compile i18n/en.json -o i18n/en.nxc                # JSON → binary catalog
```

`extract` reads the key table from the LOWERED IR, so it cannot drift from execution, and
keeps translations already present in the output file. `compile` emits the deterministic
`NXC1` binary that `Catalog::from_binary` loads; empty (untranslated) entries are omitted so
the locale chain falls through instead of rendering `""`. See [i18n.md](i18n.md).

### Generators

```bash
nx-dsl init myapp                # store + page + routes: builds out of the box
nx-dsl add page Settings myapp   # myapp/ui/pages/Settings.nx
nx-dsl add component Chip myapp  # myapp/ui/components/Chip.nx
nx-dsl add store Session myapp   # myapp/ui/composables/session.store.nx (SessionStore, SessionEvent)
nx-dsl add native <appdir>       # companion crate skeleton under <appdir>/native/
```

`add` takes an UpperCamelCase alphanumeric name and defaults the directory to `.`; every
generator refuses to overwrite an existing file. Generated sources are in canonical
format. `add native` needs an `<appdir>/manifest.toml` and writes
`native/surface.toml` (the one declaration of the companion service's methods — each
becomes `svc.<app>.<method>()` in the checker, see [services.md](services.md)), a
`Cargo.toml` and `src/lib.rs`.

## Diagnostics

Every diagnostic has a **stable code** `NX####` and a byte span printed as
`file:line:col: error[NX####]: message`. Codes are never renumbered or reused; a retired code
stays retired (`NX0409`). `nx-dsl explain <code>` is the catalog.

| Range | Phase |
|---|---|
| `NX00xx` | lexing: bad character, unterminated string, size and range bounds |
| `NX01xx` | parsing: unexpected token, duplicate property, nesting depth (64), empty `match`, route paths |
| `NX02xx` | resolution: unknown or duplicate names, widgets, modifiers, events, services, slots |
| `NX03xx` | types and shapes: arity, unknown field or case, non-exhaustive `reduce`/`match`, unknown type |
| `NX04xx` | lints: keys, labels, reducer purity, unhandled service results, queries, slots, overlays, nested blur |
| `NX05xx` | lowering: a construct outside the supported subset |

`NX0406` (a `device.profile` branch without a final `else`) is the one warning; it passes
unless `--deny-warn` is given. Everything else is an error.

## Determinism contract

- `fmt` is a fixpoint: `fmt(parse(fmt(x))) == fmt(x)`;
- building twice yields byte-identical `.nxir`; declaration order in the source never
  changes the IR;
- `.nxir.json` is a derived view for debugging, never consumed at runtime.

Both are tested ([testing.md](testing.md)); the IR rules are in [ir.md](ir.md).
