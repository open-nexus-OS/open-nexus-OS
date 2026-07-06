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
        eprintln!("usage: nx-dsl add page|component|store <Name> [dir]");
        return ExitCode::from(2);
    };
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
