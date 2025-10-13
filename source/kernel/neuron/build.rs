// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn emit_symbol_table() {
    // Only attempt in release builds; keep host/dev simple.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }
    // Output dir for generated file
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out_dir).join("trap_symbols.rs");
    // If caller provided a pre-generated map, accept it; otherwise, skip (no hard dep on tools)
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
            out.push_str("];\n");
            let _ = std::fs::write(out_path, out);
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=NEURON_LINKER_SCRIPT");
    if let Ok(script) = std::env::var("NEURON_LINKER_SCRIPT") {
        println!("cargo:rustc-link-arg=-T{script}");
    }
    // Also allow external map file
    println!("cargo:rerun-if-env-changed=NEURON_SYMBOLS_MAP");
    emit_symbol_table();
}
