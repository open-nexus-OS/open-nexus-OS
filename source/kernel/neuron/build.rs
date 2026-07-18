// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// Build scripts fail by panicking (unwrap/expect) — the correct failure mode
// for build-time codegen; the restriction lints target runtime code only.
#![allow(clippy::expect_used, clippy::unwrap_used)]

//! CONTEXT: Kernel build script – emits optional trap symbol table and configures linker/script
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! PUBLIC API: cargo build-script outputs (env/cfg, generated trap_symbols.rs)
//! DEPENDS_ON: env vars NEURON_SYMBOLS_MAP, NEURON_LINKER_SCRIPT, EMBED_INIT_ELF
//! INVARIANTS: No-op in debug; best-effort generation only
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

fn emit_symbol_table() {
    // Only attempt in release builds; keep host/dev simple.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }
    // Output dir for generated file
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out_dir).join("trap_symbols.rs");
    // Best-effort: if a map is provided and readable, generate entries; otherwise, emit empty table
    let mut generated = false;
    if let Ok(map_file) = std::env::var("NEURON_SYMBOLS_MAP") {
        if let Ok(raw) = std::fs::read_to_string(map_file) {
            let mut entries = Vec::new();
            for line in raw.lines() {
                // Expect lines like: 0000000080200abc T function_name
                let parts: Vec<_> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    if let Ok(addr) = usize::from_str_radix(parts[0].trim_start_matches('0'), 16) {
                        let name = parts[2];
                        entries.push((addr, name.to_string()));
                    }
                }
            }
            entries.sort_by_key(|e| e.0);
            let mut out = String::from(
                "#[allow(dead_code)]\npub static TRAP_SYMBOLS: &[(usize, &str)] = &[\n",
            );
            for (addr, name) in entries {
                out.push_str(&format!("    (0x{:x}, \"{}\"),\n", addr, name));
            }
            out.push_str("]\n");
            let _ = std::fs::write(&out_path, out);
            generated = true;
        }
    }
    if !generated {
        let _ = std::fs::write(
            &out_path,
            "#[allow(dead_code)]\npub static TRAP_SYMBOLS: &[(usize, &str)] = &[];\n",
        );
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Also allow external map file
    println!("cargo:rerun-if-env-changed=NEURON_SYMBOLS_MAP");
    // RFC-0068: the diag GROUP expand (`group_expanded`) reads NEXUS_LOG_EXPAND via option_env! —
    // rebuild the kernel when it changes so `NEXUS_LOG_EXPAND=syscalls` takes effect.
    println!("cargo:rerun-if-env-changed=NEXUS_LOG_EXPAND");
    emit_symbol_table();

    // Propagate optional embedded init ELF path into the kernel build so it can be included.
    println!("cargo:rerun-if-env-changed=EMBED_INIT_ELF");
    println!("cargo::rustc-check-cfg=cfg(embed_init)");
    if let Ok(path) = std::env::var("EMBED_INIT_ELF") {
        println!("cargo:rustc-cfg=embed_init");
        println!("cargo:rustc-env=EMBED_INIT_ELF={path}");
        // Rebuild if the embedded ELF changes
        println!("cargo:rerun-if-changed={path}");
    }
}
