# smoltcp (vendored)

This directory contains a **vendored fork** of upstream `smoltcp` used by Open Nexus OS.

## Why is this vendored?

Our repository keeps `cargo-deny` strict (no duplicate crate majors). Upstream `smoltcp` 0.10 depends on `bitflags` 1.x, while the rest of the workspace uses `bitflags` 2.x. This leads to duplicate majors (and type/API split) in the dependency graph.

To keep the graph single-major and deterministic, we vendor a copy of `smoltcp` and apply a small patch set.

## Upstream base

- **Crate**: `smoltcp`
- **Version**: `0.10.0`
- **License**: `0BSD` (see `LICENSE-0BSD.txt`)
- **Upstream repo**: `https://github.com/smoltcp-rs/smoltcp`

## Local deltas (Open Nexus OS)

- **Dependency hygiene**:
  - `bitflags = "2"` (instead of upstream `1.x`)
  - `defmt` dependency/feature removed (avoids pulling `bitflags 1.x` back via `defmt`)

> Policy: keep the fork minimal. Prefer upstreaming or dropping deltas when upstream catches up.

## How this is wired

Workspace root `Cargo.toml` uses:

```toml
[patch.crates-io]
smoltcp = { path = "vendor/smoltcp" }
```

This overrides crates.io `smoltcp` for the whole workspace.

## Updating

1. Replace the contents of this directory with the desired upstream release.
2. Re-apply the minimal deltas above.
3. Run:
   - `just deny-check` (must be clean)
   - `just test-all`

