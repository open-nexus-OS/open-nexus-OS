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

type DynError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), DynError> {
    println!("cargo:rerun-if-changed=link.ld");
    let out = path_buf_from_env("OUT_DIR")?;
    let dst = out.join("link.ld");
    std::fs::copy("link.ld", &dst)
        .map_err(|err| format!("copy link.ld -> {}: {err}", dst.display()))?;
    // IMPORTANT:
    // init-lite is an OS-only binary; however, `cargo test --workspace` builds and *runs* a host
    // test harness for every bin target. If we pass an OS linker script on the host, the binary
    // can be linked at invalid addresses and crash at runtime (SIGSEGV).
    //
    // So: only apply OS-only linker/config when we are actually building the OS target.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_os_target = target_arch == "riscv64" && target_os == "none";
    if is_os_target {
        println!("cargo:rustc-link-arg=-T{}", dst.display());
        println!("cargo:rustc-check-cfg=cfg(nexus_env, values(\"os\"))");
        println!("cargo:rustc-cfg=nexus_env=\"os\"");
    }
    println!("cargo:rerun-if-env-changed=INIT_LITE_LOG_TOPICS");
    match std::env::var("INIT_LITE_LOG_TOPICS") {
        Ok(spec) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS={spec}"),
        Err(_) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS="),
    }
    println!("cargo:rerun-if-env-changed=INIT_LITE_FORCE_PROBE");
    match std::env::var("INIT_LITE_FORCE_PROBE") {
        Ok(val) => println!("cargo::rustc-env=INIT_LITE_FORCE_PROBE={val}"),
        Err(_) => println!("cargo::rustc-env=INIT_LITE_FORCE_PROBE="),
    }

    generate_service_table(&out)?;
    Ok(())
}

fn generate_service_table(out: &std::path::Path) -> Result<(), DynError> {
    use object::{Object, ObjectSection, ObjectSymbol};

    let mut services = Vec::new();
    let manifest_env = std::env::var("INIT_LITE_SERVICE_LIST").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=INIT_LITE_SERVICE_LIST");

    let default_candidates = [
        "keystored",
        "identityd",
        "rngd",
        "policyd",
        "logd",
        "samgrd",
        "bundlemgrd",
        "updated",
        "packagefsd",
        "vfsd",
        "execd",
    ];

    // If no manifest is provided, auto-discover services for which ELFs are present.
    let manifest_iter: Box<dyn Iterator<Item = &str>> = if manifest_env.trim().is_empty() {
        Box::new(default_candidates.iter().copied())
    } else {
        Box::new(manifest_env.split(',').map(|s| s.trim()))
    };

    for name in manifest_iter {
        if name.is_empty() {
            continue;
        }
        let upper = name.to_ascii_uppercase().replace('-', "_");
        let path_var = format!("INIT_LITE_SERVICE_{}_ELF", upper);
        println!("cargo:rerun-if-env-changed={}", path_var);
        let src_path = match std::env::var(&path_var) {
            Ok(v) => v,
            Err(_) => {
                // Skip services whose ELFs are not provided when using the default list.
                if manifest_env.trim().is_empty() {
                    continue;
                }
                return Err(format!(
                    "missing {path_var} while building init-lite (service '{name}')"
                )
                .into());
            }
        };
        // IMPORTANT: the build script must rerun whenever the service ELF changes,
        // otherwise init-lite can embed stale copies even though the service crates were rebuilt.
        println!("cargo:rerun-if-changed={}", src_path);

        let stack_var = format!("INIT_LITE_SERVICE_{}_STACK_PAGES", upper);
        println!("cargo:rerun-if-env-changed={}", stack_var);
        let stack_pages =
            std::env::var(&stack_var).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(8);

        let dest = out.join(format!("service-{}.elf", name));
        std::fs::copy(&src_path, &dest).map_err(|err| {
            format!("failed to copy service ELF {} -> {}: {err}", src_path, dest.display())
        })?;

        let elf_bytes = std::fs::read(&dest).map_err(|err| {
            format!("failed to read copied service ELF {}: {err}", dest.display())
        })?;
        let global_pointer = {
            let file = object::File::parse(&*elf_bytes)
                .map_err(|err| format!("parse service ELF {}: {err}", dest.display()))?;
            match file.symbols().find_map(|symbol| {
                symbol
                    .name()
                    .ok()
                    .filter(|sym| *sym == "__global_pointer$")
                    .map(|_| symbol.address())
            }) {
                Some(addr) => addr,
                None => {
                    // Release artifacts for the bare-metal target may be stripped (no `.symtab`).
                    // Derive gp from small-data base per RISC-V psABI convention:
                    // `gp = __sdata_begin + 0x800`.
                    let Some(sdata) = file.section_by_name(".sdata") else {
                        return Err(format!(
                            "service {name} missing __global_pointer$ and .sdata section (cannot derive gp)"
                        )
                        .into());
                    };
                    let gp = sdata.address().saturating_add(0x800);
                    println!(
                        "cargo:warning=init-lite: service {name} missing __global_pointer$; using gp=.sdata+0x800 (0x{gp:x})"
                    );
                    gp
                }
            }
        };

        services.push(ServiceBuildEntry {
            name: name.to_string(),
            file: format!("service-{}.elf", name),
            stack_pages,
            global_pointer,
        });
    }

    let generated = out.join("services.rs");
    let mut file = std::fs::File::create(&generated)
        .map_err(|err| format!("create {}: {err}", generated.display()))?;
    use std::io::Write as _;
    let service_count = services.len();
    writeln!(
        file,
        "use nexus_init::os_payload::ServiceImage;

pub const SERVICE_IMAGE_TABLE: [ServiceImage; {count}] = [",
        count = service_count
    )?;

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
        )?;
    }

    writeln!(
        file,
        "];

pub const SERVICE_IMAGES: &[ServiceImage] = &SERVICE_IMAGE_TABLE;"
    )?;
    Ok(())
}

struct ServiceBuildEntry {
    name: String,
    file: String,
    stack_pages: u64,
    global_pointer: u64,
}

fn path_buf_from_env(key: &str) -> Result<std::path::PathBuf, DynError> {
    std::env::var(key)
        .map(std::path::PathBuf::from)
        .map_err(|_| format!("missing env var {key}").into())
}
