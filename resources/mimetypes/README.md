# Mimetype icons — file-type artwork + the platform mime SSOT

File-type icons for file listings (stash and any future file surface), plus **`mimetypes.toml`**,
the single source of truth mapping `extension → mime → icon stem` (contract:
`docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md`; consumer wiring:
`tasks/TASK-0294-mime-ssot-nexus-mime-icons-stash-filetype-icons.md`).

## Naming rule

- One SVG per icon stem, lowercase: **mime type with `/` and `+` replaced by `-`**
  (`image/jpeg` → `image-jpeg.svg`, `image/svg+xml` → `image-svg-xml.svg`).
- Class generics use the freedesktop pattern `<class>-x-generic.svg`
  (`audio-x-generic.svg`, …) plus `inode-directory.svg` and `application-octet-stream.svg`
  (the "unknown" card).
- Extension-branded variants (artwork carrying a format label where a plainer generic also
  exists) are suffixed with the extension: `text-plain-txt.svg` (the "TXT"-labelled card;
  `text-plain.svg` is the unlabelled generic). Both kept deliberately — specific art for common
  extensions, generic art as the fallback tier.

## Resolution chain (normative — keep in sync with RFC-0073)

1. extension → mime via `mimetypes.toml` `[types.<ext>]`
2. explicit `icon` override in that entry, if set
3. derived stem (`mime` with `/`,`+` → `-`) if the SVG exists
4. `[fallbacks].<mime class>`
5. `[fallbacks].unknown`

Directories always resolve to `inode-directory`. Every SVG here must be reachable through the
TOML (override, derived, or fallback) — the `nexus-mime-icons` bake enforces this both ways.

## Artwork style

- Canvas `51×47`, card `rect x=0.5 y=0.5 w=50 h=46 rx=4.5` with a class color fill and
  `stroke="#E4E4E4"`; pictogram/label in white on top.
- Labelled cards put the format name (as **outlined vector paths**, never `<text>`) in the upper
  area and the pictogram at y≈21–38; label-less cards center the pictogram.
- Class colors in use: images `#78C5EE` (+ per-format brands), video `#F16690`, audio/archives
  `#8252B1`, text/generic `#C4C4C4`, code/config `#F1A02E` (+ language brands), executables/gears
  `#3862E8`, spreadsheet `#00B48D`, presentation `#F57900`.
- Renderer budget: the icons are rasterized by `nexus-svg` (TASK-0294) — **no `<use>`, no
  `<defs>`, no radial gradients, no `<text>`**. `application-pdf.svg` and `package-x-generic.svg`
  originally shipped a bounding-box `<g clip-path>`/`<defs><clipPath>` wrapper that nexus-svg
  cannot render; both were flattened (wrapper + `defs` removed, inner paths kept — the clip was a
  no-op) when the bake landed. All 39 stems now rasterize cleanly.

## Provenance

- 27 original cards (curated set, 2026-07-15, normalized from mixed naming to the rule above).
- 12 gap-fill cards added 2026-07-15 in the same style (pictogram-only, label-less tier):
  `x-office-spreadsheet`, `x-office-presentation`, `image-svg-xml`, `text-markdown`,
  `application-json`, `text-x-python`, `application-x-tar`, `application-gzip`,
  `font-x-generic`, `application-x-executable`, `application-yaml`, `application-toml`.
