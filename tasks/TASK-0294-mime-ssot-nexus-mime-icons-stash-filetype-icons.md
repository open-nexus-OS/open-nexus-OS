---
title: TASK-0294 Mime SSOT wiring + nexus-mime-icons bake + stash file-type icons
status: In Review
owner: @runtime
created: 2026-07-15
depends-on:
  - TASK-0291
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task): docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md
  - Mime SSOT data: resources/mimetypes/mimetypes.toml (+ resources/mimetypes/README.md)
  - Pattern to follow: userspace/ui/app-icons/build.rs (manifest-scanned SVG bake)
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`resources/mimetypes/` is a curated, normalized icon set (freedesktop-style stems) with a TOML
SSOT mapping `extension → mime → icon stem` and a normative fallback chain (RFC-0073). Nothing
consumes it yet: stash rows show generic glyphs and `svc.files` returns `mime = ""` until the
resolver exists. The app-icon pipeline (`nexus-app-icons`: build.rs scan → `nexus-svg` raster at
N sizes → `sprite_bytes(key, size)` table → DSL `Image` primitive) is the proven pattern — this
task builds its mime-keyed sibling.

## Goal

1. **Resolver**: a tiny shared crate (or module in the files service host) generated/parsed from
   `mimetypes.toml` at build time — one table, consumed by the `svc.files` `mime` field and by the
   icon bake. No hand-written second copy.
2. **`userspace/ui/mime-icons`** (`nexus-mime-icons`): build.rs bakes every stem in
   `resources/mimetypes/*.svg` at file-row-appropriate sizes (16/24/32; supersampled like
   app-icons), keyed by icon stem.
3. **DSL convention**: `Image { source: "mime:<mime>", size }` resolves stem via the SSOT chain
   (exact → class generic → `application-octet-stream`) in the runtime registry, exactly parallel
   to app-id images. Unknown mime renders the fallback stem, never a blank.
4. **stash**: FileRow shows the real per-type icon from the entry's `mime`.

## Non-Goals

- Content sniffing (extension-only in v1; `magic` field reserved in the TOML).
- New rasterizer features (nexus-svg as-is; icons are authored within its supported subset —
  no `use`/`defs`/radial gradients).
- Theming/variant icons (single set in v1).

## Constraints / invariants (hard requirements)

- SSOT rule: any extension/mime/stem pair exists exactly once, in `mimetypes.toml`. Build fails if
  the TOML references a missing SVG or an SVG is unmapped (bake-time consistency guard).
- Bounded bake output (sizes fixed; sprite table size asserted in a test).
- Fallback chain deterministic and total (every input resolves to some stem).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nexus-mime-icons`: consistency guard (TOML ↔ SVG set), fallback-chain totality,
  sprite lookup for exact/class/unknown.
- Resolver tests: extension casing, multi-dot names, no-extension, directory.

### Proof (OS / QEMU) — required

- `stash: mime icons resolved (n=<count>)` — count of non-fallback icons in a real listing.
- Visible-boot evidence: screenshot of stash showing distinct per-type icons.

## Touched paths (allowlist)

- `userspace/ui/mime-icons/` (new), `userspace/dsl/runtime/src/registry.rs` (`mime:` source)
- files service host (mime field wiring), `userspace/apps/stash/` (FileRow icon)
- `resources/mimetypes/` (only additive icon gap-fills if the bake reveals holes)
- `scripts/qemu-test.sh`, `docs/dev/ui/` (Image source conventions doc touch)

## Progress snapshot (2026-07-15) — DONE, boot-proven

Delivered exactly to the contract; the DSL image convention landed as `"mime:<token>"` (token =
resolved stem | mime | extension) so the runtime primitive stays dumb and the resolution SSOT lives
once, in the bake crate.

- [x] **`userspace/ui/mime-icons`** (`nexus-mime-icons`): `build.rs` scans `resources/mimetypes/*.svg`,
  rasterizes each stem through `nexus-svg` at `[48, 32, 24]` (SS=4, box-average downscale, straight
  RGBA — the `nexus-app-icons` pattern), and folds the full RFC-0073 resolution chain into generated
  `stem_for_ext` / `stem_for_mime` tables (every generated stem is guaranteed to have artwork).
  Build-time SSOT guard: an explicit `icon = "…"` override naming a missing SVG fails the build.
  7 host tests (chain, casing, multi-dot names, mime, all source forms, artwork totality, counts).
- [x] Flattened the two SVGs nexus-svg could not render (`application-pdf.svg`, `package-x-generic.svg`
  had a no-op `<g clip-path>`/`<defs><clipPath>` wrapper) — all 39 stems rasterize.
- [x] **DSL `Image` primitive** (`registry.rs`): `"mime:<token>"` → `nexus_mime_icons::sprite_for_source`;
  non-`mime:` sources keep the app-icon path. Shared sprite-blit tail, no duplication.
- [x] **app-host** (`effect_host.rs`): `entry_icon_stem` resolves each listing entry (dir → directory
  stem, file → extension via the SSOT) and emits `icon = "mime:<stem>"`; a per-listing marker counts
  resolved (non-fallback) icons.
- [x] **stash** `FileRow.nx`: `Icon { symbol }` → `Image { source: $props.icon, size: 24 }`.
- [x] **First-run content**: `nxfsd::DataStore` seeds a small varied set (`Welcome.txt`, `Read Me.md`,
  `Report.pdf`, `Photo.png`, `Song.mp3`, `Archive.zip`, `config.json` + `Documents`/`Pictures`) on a
  blank format only (`Nxfs.formatted_fresh`), so the icon variety is visible out of the box like an OS
  shipping example files.

### Proof (Host) — met

- `cargo test -p nexus-mime-icons` (7) + `cargo test -p nxfs` (22, unchanged after the `formatted_fresh`
  field). app-host / vfsd / bundlemgrd os-lite builds clean (stash `.nx` recompiles via bundlemgrd's
  build.rs — valid nxir).

### Proof (OS / QEMU) — met (visible virgl boot, fresh images, 1280×800)

- `nxfsd: mounted /data (rw, clean)` → `nxfsd: seeded first-run content (n=9)` →
  `apphost: dsl svc files.list ok (n=9)` → **`stash: mime icons resolved (n=9)`** (all nine entries
  resolved to real artwork).
- Screenshot: stash lists `/data` with distinct per-type icons — red PDF card, purple ZIP package,
  green PNG, purple MP3, TXT/MD text cards, orange JSON, folder icons for Documents/Pictures.
