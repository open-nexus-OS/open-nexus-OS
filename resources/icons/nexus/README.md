# The project's own glyphs

Glyphs the design artifacts draw that no file of the vendored icon set has, drawn on the vendor
set's grid (24 × 24) and with its stroke (`stroke-width="2"`, round caps and joins), so the icon
build strokes them exactly like the vendor's. The base theme names this folder `[icons] own`; a
symbol whose file is `nexus:<stem>` resolves here (`resources/themes/base.nxtheme.toml`).

| File | Symbol | What it is |
|---|---|---|
| `launcher.svg` | `launcher` | the taskbar's launcher: four dots and a search ring with its handle (design artifacts: TaskbarDock). A filled dot of radius 2 is a circle of radius 1 stroked at 2 — the build strokes, it does not fill. |

Licensed like the repository (Apache-2.0).
