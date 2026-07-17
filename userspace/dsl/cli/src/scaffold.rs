// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx-dsl` generators — `init` (a minimal buildable app skeleton)
//! and `add page|component|store` (one canonical file each). Generated
//! sources are canonical-format `.nx` (fmt-stable) and the init skeleton
//! builds green out of the box (scaffold test pins this).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/dsl_v0_2_host (init → build green)

use std::path::Path;
use std::process::ExitCode;

/// `init <dir>` — a minimal app: store, page, routes.
pub fn cmd_init(args: &[String]) -> ExitCode {
    let Some(dir) = args.first() else {
        eprintln!("usage: nx-dsl init <dir>");
        return ExitCode::from(2);
    };
    let files: &[(&str, &str)] = &[
        (
            "ui/composables/app.store.nx",
            "Store AppStore {\n    title: Str = \"Hello\",\n    count: Int = 0,\n}\n\n\
             Event AppEvent {\n    Increment,\n}\n\n\
             reduce AppEvent {\n    Increment => state.count += 1,\n}\n",
        ),
        (
            "ui/pages/MainPage.nx",
            "Page MainPage {\n    Stack {\n        Text($state.title).textSize(lg)\n        \
             Button { label: \"Count\" }\n        on Tap -> dispatch(Increment)\n    }\n    \
             .padding(4)\n    .gap(2)\n}\n",
        ),
        ("ui/pages/Routes.nx", "Routes {\n    \"/\" -> MainPage;\n}\n"),
    ];
    for (rel, content) in files {
        let path = Path::new(dir).join(rel);
        if path.exists() {
            eprintln!("nx-dsl init: `{}` already exists — refusing to overwrite", path.display());
            return ExitCode::from(1);
        }
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("nx-dsl: cannot create `{}`: {e}", parent.display());
                return ExitCode::from(2);
            }
        }
        if let Err(e) = std::fs::write(&path, content) {
            eprintln!("nx-dsl: cannot write `{}`: {e}", path.display());
            return ExitCode::from(2);
        }
    }
    println!("{dir}: app skeleton created ({} files)", files.len());
    ExitCode::SUCCESS
}

/// `add page|component|store <Name> [dir]` — one canonical file.
pub fn cmd_add(args: &[String]) -> ExitCode {
    let (Some(kind), Some(name)) = (args.first(), args.get(1)) else {
        eprintln!("usage: nx-dsl add page|component|store <Name> [dir] | add native <appdir>");
        return ExitCode::from(2);
    };
    if kind == "native" {
        // `add native <appdir>`: `name` IS the app directory.
        return add_native(std::path::Path::new(name));
    }
    if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        || !name.chars().all(|c| c.is_ascii_alphanumeric())
    {
        eprintln!("nx-dsl add: `{name}` must be UpperCamelCase alphanumeric");
        return ExitCode::from(1);
    }
    let dir = args.get(2).map(String::as_str).unwrap_or(".");
    let (rel, content) = match kind.as_str() {
        "page" => (
            format!("ui/pages/{name}.nx"),
            format!(
                "Page {name} {{\n    Stack {{\n        Text(\"{name}\")\n    }}\n    .padding(4)\n}}\n"
            ),
        ),
        "component" => (
            format!("ui/components/{name}.nx"),
            format!(
                "Component {name} {{\n    props: {{\n        label: Str,\n    }}\n    \
                 Stack {{\n        Text($props.label)\n    }}\n}}\n"
            ),
        ),
        "store" => (
            format!("ui/composables/{}.store.nx", name.to_lowercase()),
            format!(
                "Store {name}Store {{\n    value: Int = 0,\n}}\n\n\
                 Event {name}Event {{\n    Changed(Int),\n}}\n\n\
                 reduce {name}Event {{\n    Changed(v) => state.value = v,\n}}\n"
            ),
        ),
        other => {
            eprintln!("nx-dsl add: unknown kind `{other}` (page|component|store)");
            return ExitCode::from(2);
        }
    };
    let path = Path::new(dir).join(&rel);
    if path.exists() {
        eprintln!("nx-dsl add: `{}` already exists — refusing to overwrite", path.display());
        return ExitCode::from(1);
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("nx-dsl: cannot create `{}`: {e}", parent.display());
            return ExitCode::from(2);
        }
    }
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("nx-dsl: cannot write `{}`: {e}", path.display());
        return ExitCode::from(2);
    }
    println!("{}", path.display());
    ExitCode::SUCCESS
}

/// `nx-dsl add native <appdir>` (TASK-0081 C1): scaffolds the app's
/// companion crate — `native/surface.toml` (the ONE surface declaration the
/// checker reads on every project build), a minimal Cargo.toml (SDK-curated
/// deps only, see docs/dev/sdk/crates.md), and a server skeleton whose
/// trait mirrors the surface. Re-running on an existing `native/` refuses
/// (never overwrites developer code).
fn add_native(app_root: &std::path::Path) -> ExitCode {
    if !app_root.join("manifest.toml").is_file() {
        eprintln!(
            "nx-dsl add native: {} has no manifest.toml (run from userspace/apps/<name>)",
            app_root.display()
        );
        return ExitCode::from(1);
    }
    let Some(app) = app_root.file_name().and_then(|n| n.to_str()).map(String::from) else {
        eprintln!("nx-dsl add native: app dir has no name");
        return ExitCode::from(1);
    };
    let native = app_root.join("native");
    if native.exists() {
        eprintln!("nx-dsl add native: {} already exists (refusing to overwrite)", native.display());
        return ExitCode::from(1);
    }
    if let Err(err) = std::fs::create_dir_all(native.join("src")) {
        eprintln!("nx-dsl add native: mkdir: {err}");
        return ExitCode::from(1);
    }
    let mut surface = String::new();
    surface.push_str("# Companion service surface (TASK-0081 C1) — the SSOT the DSL checker\n");
    surface
        .push_str("# reads on every project build: each method appears as `svc.<app>.<name>()`.\n");
    surface.push_str("# Types are DSL types (Str, Int, Bool, Fx, List<...>).\n");
    surface.push_str("[[method]]\nname = \"ping\"\nargs = [\"Str\"]\nresult = \"Str\"\n");
    let mut cargo = String::new();
    cargo.push_str(&format!(
        "# Companion crate for `{app}` (its OWN process, its OWN manifest caps —\n"
    ));
    cargo.push_str("# TASK-0081 C1). Dependencies: SDK-curated crates only\n");
    cargo.push_str("# (docs/dev/sdk/crates.md); raw syscall/IPC layers are the trust boundary.\n");
    cargo.push_str(&format!("[package]\nname = \"{app}-native\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"Apache-2.0\"\n\n[lib]\npath = \"src/lib.rs\"\n"));
    let mut lib = String::new();
    lib.push_str("// Copyright 2026 Open Nexus OS Contributors\n");
    lib.push_str("// SPDX-License-Identifier: Apache-2.0\n\n");
    lib.push_str(&format!(
        "//! CONTEXT: `{app}` companion service (scaffolded by `nx dsl add native`,\n"
    ));
    lib.push_str("//! TASK-0081 C1): implements the surface declared ONCE in `../surface.toml`\n");
    lib.push_str(&format!(
        "//! — the DSL app calls it as `svc.{app}.<method>()`. Runs as its OWN\n"
    ));
    lib.push_str("//! process with its OWN manifest capabilities (spawn wiring rides with the\n");
    lib.push_str("//! companion runtime step).\n");
    lib.push_str(&format!("//! OWNERS: @{app}\n"));
    lib.push_str("//! STATUS: Experimental\n//! API_STABILITY: Unstable\n");
    lib.push_str("//! TEST_COVERAGE: add unit tests alongside your implementation\n\n");
    lib.push_str("/// The service surface — one method per `[[method]]` in surface.toml.\n");
    lib.push_str("/// KEEP IN SYNC: the checker enforces the DSL side from surface.toml;\n");
    lib.push_str("/// this trait is the Rust side of the same contract.\n");
    lib.push_str("pub trait Surface {\n");
    lib.push_str(&format!("    /// `svc.{app}.ping(text)` — replace with your real methods.\n"));
    lib.push_str("    fn ping(&mut self, text: &str) -> Result<String, u32>;\n");
    lib.push_str("}\n");
    for (rel, content) in [("surface.toml", surface), ("Cargo.toml", cargo), ("src/lib.rs", lib)] {
        if let Err(err) = std::fs::write(native.join(rel), content) {
            eprintln!("nx-dsl add native: write {rel}: {err}");
            return ExitCode::from(1);
        }
    }
    println!("nx-dsl add native: scaffolded {}", native.display());
    println!("  next: declare real methods in native/surface.toml, implement the");
    println!("  Surface trait, and keep deps SDK-curated (docs/dev/sdk/crates.md).");
    ExitCode::SUCCESS
}
