// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `nexus-mime-icons` — the baked file-type icon artwork plus the
//! mime resolution tables. The mime SSOT (`resources/mimetypes/mimetypes.toml`,
//! RFC-0073) maps `extension → mime → icon stem`; the build script folds the
//! full resolution chain (exact → derived → class-generic → octet-stream) in at
//! build time and rasterizes each stem's SVG through `nexus-svg` into
//! straight-alpha RGBA sprites at file-row sizes. Runtime is a pure table
//! lookup — no parsing, no allocation, `no_std`.
//!
//! Two consumers share this one crate so the SSOT is never duplicated: the
//! app-host resolves a listing entry's name/kind to a stem (`stem_for_file_name`
//! / [`DIRECTORY`]) and emits `icon = "mime:<stem>"`; the DSL `Image` primitive
//! turns a `"mime:<token>"` source into a sprite via [`sprite_for_source`].
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: resolution chain + sprite presence + filename extraction

mod baked {
    include!(concat!(env!("OUT_DIR"), "/mime_icons.rs"));
}

/// Ultimate fallback stem (a real SVG); returned for anything unrecognised.
pub const UNKNOWN: &str = "application-octet-stream";
/// The stem every directory uses (the caller supplies the is-dir semantics).
pub const DIRECTORY: &str = "inode-directory";

/// One baked file-type sprite: `size × size` straight-alpha RGBA rows.
#[derive(Debug, Clone, Copy)]
pub struct MimeIconSprite {
    pub size: u32,
    /// `size * size * 4` bytes, `[r, g, b, a]` per pixel, row-major.
    pub rgba: &'static [u8],
}

/// The baked sprite for an icon stem at an exact baked size, or the LARGEST
/// baked size as a fallback (the painter samples nearest, so any box size
/// renders). `None` = no artwork for that stem.
#[must_use]
pub fn sprite(stem: &str, size: u32) -> Option<MimeIconSprite> {
    if let Some(rgba) = baked::sprite_bytes(stem, size) {
        return Some(MimeIconSprite { size, rgba });
    }
    for &s in baked::BAKED_SIZES {
        if let Some(rgba) = baked::sprite_bytes(stem, s) {
            return Some(MimeIconSprite { size: s, rgba });
        }
    }
    None
}

/// Resolves a `"mime:"` source token to an icon stem. The token may be:
/// an already-resolved stem (the app-host's fast path), a mime type (contains
/// `/`), or a bare file extension. Always returns a stem that has artwork.
#[must_use]
pub fn resolve_stem(token: &str) -> &'static str {
    if token.contains('/') {
        return baked::stem_for_mime(token);
    }
    if let Some(stem) = baked::stem_static(token) {
        return stem;
    }
    baked::stem_for_ext(token)
}

/// The sprite for a `"mime:"` source token (see [`resolve_stem`]).
#[must_use]
pub fn sprite_for_source(token: &str, size: u32) -> Option<MimeIconSprite> {
    sprite(resolve_stem(token), size)
}

/// Resolves a file extension (case-insensitive) to an icon stem.
#[must_use]
pub fn stem_for_ext(ext: &str) -> &'static str {
    with_lowercased(ext, baked::stem_for_ext).unwrap_or(UNKNOWN)
}

/// Resolves a mime type to an icon stem.
#[must_use]
pub fn stem_for_mime(mime: &str) -> &'static str {
    baked::stem_for_mime(mime)
}

/// Resolves a file NAME to an icon stem by its extension (the part after the
/// last `.`). Dotfiles and extension-less names resolve to [`UNKNOWN`].
/// Directories are the caller's concern — use [`DIRECTORY`].
#[must_use]
pub fn stem_for_file_name(name: &str) -> &'static str {
    match name.rfind('.') {
        Some(dot) if dot + 1 < name.len() => stem_for_ext(&name[dot + 1..]),
        _ => UNKNOWN,
    }
}

/// Whether a stem has baked artwork.
#[must_use]
pub fn has_icon(stem: &str) -> bool {
    baked::stem_static(stem).is_some()
}

/// Runs `f` over an ASCII-lowercased copy of `s` on the stack — no allocation.
/// Extensions longer than the buffer (never real) yield `None`.
fn with_lowercased(s: &str, f: fn(&str) -> &'static str) -> Option<&'static str> {
    const MAX: usize = 16;
    if s.len() > MAX {
        return Some(UNKNOWN);
    }
    let mut buf = [0u8; MAX];
    for (i, b) in s.bytes().enumerate() {
        buf[i] = b.to_ascii_lowercase();
    }
    core::str::from_utf8(&buf[..s.len()]).ok().map(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_chain_hits_expected_stems() {
        // exact derived stem
        assert_eq!(stem_for_ext("pdf"), "application-pdf");
        assert_eq!(stem_for_ext("png"), "image-png");
        assert_eq!(stem_for_ext("rs"), "text-x-rust");
        // icon override wins
        assert_eq!(stem_for_ext("xlsx"), "x-office-spreadsheet");
        assert_eq!(stem_for_ext("ts"), "text-javascript");
        assert_eq!(stem_for_ext("nx"), "text-x-script");
        // class-generic fallback (no exact video-quicktime.svg)
        assert_eq!(stem_for_ext("mov"), "video-x-generic");
        assert_eq!(stem_for_ext("opus"), "audio-x-generic");
        // unknown extension → octet-stream
        assert_eq!(stem_for_ext("zzz"), UNKNOWN);
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(stem_for_ext("PDF"), "application-pdf");
        assert_eq!(stem_for_ext("Png"), "image-png");
    }

    #[test]
    fn file_name_extraction() {
        assert_eq!(stem_for_file_name("report.PDF"), "application-pdf");
        assert_eq!(stem_for_file_name("archive.tar.gz"), "application-gzip");
        assert_eq!(stem_for_file_name("Makefile"), UNKNOWN);
        assert_eq!(stem_for_file_name(".bashrc"), UNKNOWN);
    }

    #[test]
    fn mime_resolution() {
        assert_eq!(stem_for_mime("image/jpeg"), "image-jpeg");
        assert_eq!(stem_for_mime("application/pdf"), "application-pdf");
        assert_eq!(stem_for_mime("application/x-unheard-of"), UNKNOWN);
    }

    #[test]
    fn source_token_forms_all_resolve() {
        // already-resolved stem (app-host fast path)
        assert!(sprite_for_source("application-pdf", 24).is_some());
        // bare extension
        assert!(sprite_for_source("pdf", 24).is_some());
        // full mime
        assert!(sprite_for_source("application/pdf", 24).is_some());
        // directory stem
        assert!(sprite_for_source(DIRECTORY, 24).is_some());
    }

    #[test]
    fn every_resolved_stem_has_artwork() {
        // The build folds the chain to existing stems; the ultimate fallback
        // and the directory stem must both be baked.
        assert!(has_icon(UNKNOWN));
        assert!(has_icon(DIRECTORY));
        for size in baked::BAKED_SIZES {
            let s = sprite(UNKNOWN, *size).expect("octet-stream baked");
            assert_eq!(s.rgba.len() as u32, *size * *size * 4);
        }
    }

    #[test]
    fn provenance_counts_are_sane() {
        assert!(baked::STEM_COUNT >= 30, "expected the full mime icon set");
        assert!(baked::EXT_COUNT >= 60, "expected the full extension table");
    }
}
