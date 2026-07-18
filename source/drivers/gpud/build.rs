// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// Build scripts fail by panicking (unwrap/expect) — the correct failure mode
// for build-time codegen; the restriction lints target runtime code only.
#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Build-time rasterization of the boot-splash logo. The Open Nexus wordmark SVG
//! is rasterized ONCE here (host, std) into a BGRA8888 bitmap embedded in gpud, so
//! the boot loading screen composites it with zero runtime SVG cost and no pressure
//! on gpud's non-freeing bump heap. gpud has no SVG rasterizer of its own.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let svg_path =
        std::path::Path::new(&manifest).join("../../../resources/icons/logos/open-nexus.svg");
    println!("cargo:rerun-if-changed={}", svg_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    // Boot-splash logo size (wordmark aspect ≈ 3508:2481). Centered on 1280×800.
    const LOGO_W: u32 = 380;
    const LOGO_H: u32 = 269;

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let bgra_dst = std::path::Path::new(&out_dir).join("splash_logo.bgra");
    let dims_dst = std::path::Path::new(&out_dir).join("splash_logo_dims.rs");

    match std::fs::read_to_string(&svg_path).map_err(|e| e.to_string()).and_then(|svg| {
        nexus_svg::render_svg_at(&svg, LOGO_W, LOGO_H).map_err(|e| format!("{e:?}"))
    }) {
        Ok(out) => {
            std::fs::write(&bgra_dst, &out.buffer).expect("write splash_logo.bgra");
            std::fs::write(
                &dims_dst,
                format!(
                    "pub const SPLASH_LOGO_W: u32 = {};\npub const SPLASH_LOGO_H: u32 = {};\n",
                    out.width, out.height
                ),
            )
            .expect("write splash_logo_dims.rs");
        }
        Err(e) => {
            // Never fail the build over the splash: emit a 0×0 logo so the composite is a no-op.
            eprintln!("[gpud build] splash logo rasterization skipped: {e}");
            std::fs::write(&bgra_dst, []).expect("write empty splash_logo.bgra");
            std::fs::write(
                &dims_dst,
                "pub const SPLASH_LOGO_W: u32 = 0;\npub const SPLASH_LOGO_H: u32 = 0;\n",
            )
            .expect("write splash_logo_dims.rs");
        }
    }
}
