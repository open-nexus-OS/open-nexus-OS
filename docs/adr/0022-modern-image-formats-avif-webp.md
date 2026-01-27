<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0022: Modern image formats (WebP/AVIF) for wallpapers, screenshots, and thumbnail caches

## Status
Proposed

## Context
Open Nexus OS UI uses large images in a few common places:

- **Wallpapers / backgrounds** (desktop/tablet/lockscreen): typically large, high-resolution assets.
- **Screenshots / screencaps**: often shared or persisted under `/state`.
- **Thumbnail caches** (`thumbd`, window thumbnails): many small images where cache size matters.

Historically, JPEG and PNG are the common defaults:

- **JPEG** is widely supported but can be significantly larger than modern formats for the same perceptual quality.
- **PNG** is deterministic and lossless but often much larger than needed for typical screenshots/backgrounds.

Separately, our UI “glass” material uses **backdrop snapshots** (blurred downsample of background content). Larger assets increase:

- time-to-first-render for the wallpaper/background layer,
- IO and memory pressure (especially on HiDPI displays),
- storage footprint when persisting screenshots or caches.

Constraints we must keep:

- **Determinism**: host/QEMU tests should remain stable. Avoid comparisons against encoder byte output when the codec/implementation may vary; prefer **pixel-hash after decode**.
- **Build hygiene**: OS services must not pull forbidden crates (`getrandom`, `parking_lot*`) and must respect `--no-default-features --features os-lite`.
- **Security**: image decoders are a high-risk parsing surface; all decode paths must enforce strict **byte and pixel caps** and avoid `unwrap/expect` on untrusted input.

## Decision
We standardize preferred formats and testing guidance:

### 1. Wallpapers / backgrounds
- **Preferred on-disk formats**: `image/avif`, then `image/webp`.
- **Fallback**: `image/jpeg`, then `image/png`.
- Wallpaper decode must be **bounded** (max input bytes, max pixels). Large/hostile inputs must be rejected deterministically.

### 2. Screenshots / screencaps (share-to-file)
- **Preferred export format**: WebP (smaller artifacts).
- **Baseline deterministic format**: PNG remains supported and may be used for goldens and deterministic export proofs.
- Tests should validate screenshots by:
  - checking a **pixel checksum** (hash of decoded BGRA), and/or
  - validating metadata (w/h/stride) + bounded size constraints,
  - avoiding direct comparisons of encoded bytes unless the encoder settings are fully pinned and proven deterministic.

### 3. Thumbnail caches
- In-memory thumbnails remain **BGRA VMOs** (fast rendering path).
- For persisted caches (when added), **WebP** is preferred as the on-disk cache format.
- Thumb generation must remain deterministic; tests should validate by pixel checksum/goldens, not by encoded-byte identity.

### 4. MIME and extension mapping
The MIME registry (`mimed`) must recognize:

- `.webp` → `image/webp`
- `.avif` → `image/avif`

### 5. Dependency and feature gating policy
- Codec support should be introduced **host-first**, behind explicit feature gates where needed.
- For OS/QEMU targets, codecs must not violate dependency hygiene; if a codec requires non-compliant deps or unsafe FFI, it must remain **host-only** until a compliant path exists.

## Consequences
### Benefits
- Smaller wallpapers and screenshot exports reduce IO and improve perceived performance.
- Thumbnail caches become more storage-efficient without affecting render-time internal formats.

### Costs / Risks
- Additional codec complexity and dependency surface area.
- Potential nondeterminism in encoder byte output if tests rely on raw file bytes.

### Mitigations
- Keep PNG as deterministic baseline for goldens and strict proofs.
- Validate modern formats in tests via decoded pixel hashes.
- Enforce strict budgets (max bytes/pixels) and maintain decode hardening discipline.
