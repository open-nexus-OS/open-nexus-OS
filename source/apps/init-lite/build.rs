// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for init-lite application
//! OWNERS: @runtime
//! STATUS: Deprecated
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - main(): Build script entry point
//!
//! DEPENDENCIES:
//!   - std::env: Environment variables
//!   - std::fs: File system operations
//!   - link.ld: Linker script
//!
//! ADR: docs/adr/0017-service-architecture.md

fn main() {
    println!("cargo:rerun-if-changed=link.ld");
    let out = path_buf_from_env("OUT_DIR");
    let dst = out.join("link.ld");
    std::fs::copy("link.ld", &dst).expect("copy link.ld");
    println!("cargo:rustc-link-arg=-T{}", dst.display());
    println!("cargo:rustc-check-cfg=cfg(nexus_env, values(\"os\"))");
    println!("cargo:rustc-cfg=nexus_env=\"os\"");
    println!("cargo:rerun-if-env-changed=INIT_LITE_LOG_TOPICS");
    match std::env::var("INIT_LITE_LOG_TOPICS") {
        Ok(spec) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS={spec}"),
        Err(_) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS="),
    }

    generate_service_table(&out);
}

fn generate_service_table(out: &std::path::Path) {
    use object::{Object, ObjectSymbol};

    let mut services = Vec::new();
    let manifest = std::env::var("INIT_LITE_SERVICE_LIST").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=INIT_LITE_SERVICE_LIST");

    for raw in manifest.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let upper = name.to_ascii_uppercase().replace('-', "_");
        let path_var = format!("INIT_LITE_SERVICE_{}_ELF", upper);
        println!("cargo:rerun-if-env-changed={}", path_var);
        let src_path = std::env::var(&path_var).unwrap_or_else(|_| {
            panic!(
                "missing {} while building init-lite (service '{}')",
                path_var, name
            )
        });

        let stack_var = format!("INIT_LITE_SERVICE_{}_STACK_PAGES", upper);
        println!("cargo:rerun-if-env-changed={}", stack_var);
        let stack_pages = std::env::var(&stack_var)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(8);

        let dest = out.join(format!("service-{}.elf", name));
        std::fs::copy(&src_path, &dest).unwrap_or_else(|err| {
            panic!(
                "failed to copy service ELF {} -> {}: {}",
                src_path,
                dest.display(),
                err
            )
        });

        let elf_bytes = std::fs::read(&dest).unwrap_or_else(|err| {
            panic!(
                "failed to read copied service ELF {}: {}",
                dest.display(),
                err
            )
        });
        let global_pointer = {
            let file = object::File::parse(&*elf_bytes).expect("parse service ELF");
            file.symbols()
                .find_map(|symbol| {
                    symbol
                        .name()
                        .ok()
                        .filter(|name| *name == "__global_pointer$")
                        .map(|_| symbol.address())
                })
                .unwrap_or_else(|| {
                    panic!(
                        "service {} missing __global_pointer$ symbol (required for RISC-V gp)",
                        name
                    )
                })
        };

        services.push(ServiceBuildEntry {
            name: name.to_string(),
            file: format!("service-{}.elf", name),
            stack_pages,
            global_pointer,
        });
    }

    let generated = out.join("services.rs");
    let mut file = std::fs::File::create(&generated).expect("create services.rs");
    use std::io::Write as _;
    let service_count = services.len();
    writeln!(
        file,
        "use nexus_init::os_payload::ServiceImage;

pub const SERVICE_IMAGE_TABLE: [ServiceImage; {count}] = [",
        count = service_count
    )
    .unwrap();

    for entry in &services {
        writeln!(
            file,
            "    ServiceImage {{
        name: \"{name}\",
        elf: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{file}\")),
        stack_pages: {stack},
        global_pointer: 0x{gp:x},
    }},",
            name = entry.name,
            file = entry.file,
            stack = entry.stack_pages,
            gp = entry.global_pointer
        )
        .unwrap();
    }

    writeln!(
        file,
        "];

pub const SERVICE_IMAGES: &[ServiceImage] = &SERVICE_IMAGE_TABLE;"
    )
    .unwrap();
}

struct ServiceBuildEntry {
    name: String,
    file: String,
    stack_pages: u64,
    global_pointer: u64,
}

fn path_buf_from_env(key: &str) -> std::path::PathBuf {
    std::env::var(key)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| panic!("missing env var {}", key))
}
