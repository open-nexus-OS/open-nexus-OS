<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# resources/

Static assets consumed at build time (mostly via `build.rs` bakes / `include_str!`)
and by resource pipelines. Two kinds of subdirectories live here:

- **CURATED** — assets we author/select ourselves; everything in the dir is intentional.
- **VENDORED** — full upstream trees pinned as **git submodules** (`resources/fonts/inter`,
  `resources/icons/lucide`, `resources/cursors/mocu`; see `.gitmodules`); only a small
  subset is used at build time.

## Inventory

| Dir | Kind | Size / files | What it is |
|---|---|---|---|
| `fonts/` | VENDORED | ~39 MB, ~6200 files | Upstream font repos as submodules. `inter/` is the complete Inter source tree (glyph sources, docs, build tooling). `monospace/` and `noto/` are empty placeholders. |
| `icons/` | VENDORED (`lucide/`) + curated (`logos/`) | ~22 MB, ~4500 files | `lucide/` is the full upstream Lucide icon repo (submodule) (~1740 icons as `.svg` + `.json` pairs under `lucide/icons/`). `logos/` holds the project logo (`open-nexus.svg`). |
| `app-icons/` | CURATED | ~600 KB, 148 files | 48 app-icon artworks, SVG-only, baked by `nexus-app-icons`. See `app-icons/README.md`. |
| `wallpapers/` | CURATED | ~330 KB, 3 files | Wallpaper sets; currently one set `base/` with a light and a dark image. |
| `cursors/` | CURATED | ~285 KB, 68 files | The `mocu/` cursor theme (SVG sources + build script/license). Specific cursors are included per name by `windowd/build.rs`. |
| `mimetypes/` | CURATED | ~190 KB, 41 files | Platform MIME SSOT: `mimetypes.toml` (extension → mime → icon stem, contract RFC-0073) plus 39 file-type icon SVGs, baked by `nexus-mime-icons`. |
| `themes/` | CURATED | ~20 KB, 4 files | Theme token definitions (base/dark/light/highcontrast), loaded by `windowd/build.rs`. |
| `manifests/` | CURATED | ~16 KB, 4 files | Service manifests (v2.0 format: name, bundle_type, provides, dependencies). |

## Naming conventions (as observed)

- `themes/` — `<name>.nxtheme.toml` (e.g. `dark.nxtheme.toml`). `base` is the
  fallback layer; the other themes override only what varies.
- `manifests/` — `<service>.manifest.toml` (e.g. `windowd.manifest.toml`).
- `app-icons/` — `<category>/<app>.svg` plus variants `<app>.symbolic.svg` and
  `<app>.micro.svg`; categories are `games/`, `media/`, `productivity/`, `system/`.
  Every app ships all 3 variants.
- `mimetypes/` — icon files are named after the MIME type with `/` and `+`
  replaced by `-` (e.g. `image/svg+xml` → `image-svg-xml.svg`); resolution chain
  and fallbacks are documented in `mimetypes/mimetypes.toml`.
- `cursors/` — `mocu/src/svg/<css-cursor-name>.svg` (CSS cursor keyword names,
  e.g. `default.svg`, `ew-resize.svg`, `nwse-resize.svg`).
- `wallpapers/` — `<set>/<name>.<jpg|jpeg>` with an optional dark variant
  `<name>.dark.jpg` (e.g. `base/default.jpeg`, `base/default.dark.jpg`).
- `fonts/` — upstream layout is kept as-is; the build consumes
  `fonts/inter/docs/font-files/InterVariable.ttf`.
- `icons/lucide/` — upstream layout kept as-is; icons live at
  `icons/lucide/icons/<kebab-name>.svg` (paired with a `.json` metadata file).

## What the build actually uses (of the vendored dumps)

- **Fonts**: only `InterVariable.ttf` (referenced by `source/services/windowd/build.rs`);
  the rest of the ~39 MB Inter tree (glyph sources, docs, tooling) is unused ballast.
- **Lucide**: individual SVGs are pulled by name via `include_str!` in service
  `build.rs` bakes (e.g. `house.svg`, `menu.svg`, `x.svg`, `search.svg`); the vast
  majority of the ~1740 icons are never referenced.
- The project logo `icons/logos/open-nexus.svg` is used by `source/drivers/gpud/build.rs`.

## Follow-up

Pruning the vendored submodules (`fonts/inter/`, `icons/lucide/`) to a curated subset
(e.g. swapping the submodule for a checked-in asset subset)
actually consumed is planned but **deferred**: it first needs a usage analysis
across all recipes/`build.rs` bakes and `source/` references, plus a boot proof
that nothing resolves icons/fonts dynamically at runtime. Do not delete assets
ad hoc — new icons are added by name in code, so an over-eager prune breaks
future bakes silently.
