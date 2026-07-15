---
title: TASK-0294 Mime SSOT wiring + nexus-mime-icons bake + stash file-type icons
status: Draft
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
