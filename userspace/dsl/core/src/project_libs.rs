// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: library-component resolution for a project compile (TASK-0081 C3 / TASK-0074): the
//! app manifest's `dependencies` name sibling library folders; only the `Component`
//! declarations the app REFERENCES — transitively, through the library's own components — are
//! appended to the app's source set, so a kit can grow without every consumer declaring every
//! kit event. Governance (components only) is enforced here. Split out of `project.rs`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `project::widget_library_tests`

use crate::project::SourceFile;
use alloc::string::String;

/// Appends the referenced library components to `files` (see the module doc).
///
/// # Errors
/// A human-readable reason: a missing sibling folder, a library that declares more than
/// components, an unparsable component, or a dependency the app never references.
pub(crate) fn resolve_library_components(
    root: &std::path::Path,
    files: &mut alloc::vec::Vec<SourceFile>,
) -> Result<(), String> {
    // Widget libraries (TASK-0081 C3): the app manifest's `dependencies`
    // name SIBLING library folders (`userspace/apps/<lib>/`, bundleType
    // library). Their components are compiled INTO this app's one canonical
    // `.nxir` at build time — no runtime component loading, the
    // one-program-one-hash model and AOT parity stay intact. Governance is
    // enforced HERE: a library contributes ONLY `Component` declarations
    // (compositions of system primitives); anything else fails the build.
    //
    // Only the components the app REFERENCES (transitively, through the
    // library's own components) are pulled in (TASK-0074): a library grows a
    // dialog kit whose events one app declares and another never needs, and
    // compiling the whole shelf into every consumer would make each app
    // declare every kit event — or make the kit unextendable. The app names
    // what it uses; the compiler brings exactly that.
    let mut referenced: alloc::collections::BTreeSet<String> = alloc::collections::BTreeSet::new();
    for file in files.iter() {
        if let Ok(parsed) = crate::parse_file(&file.source) {
            collect_referenced_names(&parsed, &mut referenced);
        }
    }
    for dep in manifest_dependencies(root)? {
        let lib_root = root
            .parent()
            .map(|parent| parent.join(&dep))
            .filter(|p| p.join("manifest.toml").is_file())
            .ok_or_else(|| alloc::format!("dependency `{dep}`: no sibling app folder"))?;
        let components = lib_root.join("ui/components");
        let entries = std::fs::read_dir(&components).map_err(|e| {
            alloc::format!("dependency `{dep}`: read {}: {e}", components.display())
        })?;
        // (source file, its parsed declarations, the component names it declares)
        let mut shelf: alloc::vec::Vec<(SourceFile, crate::ast::File, alloc::vec::Vec<String>)> =
            alloc::vec::Vec::new();
        let mut paths: alloc::vec::Vec<std::path::PathBuf> =
            entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().and_then(|e| e.to_str()) != Some("nx") {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
            let parsed = crate::parse_file(&source)
                .map_err(|d| alloc::format!("dependency `{dep}`: parse: {d:?}"))?;
            if !parsed.decls.iter().all(|decl| matches!(decl, crate::ast::Decl::Component(_))) {
                return Err(alloc::format!(
                    "dependency `{dep}`: {} declares more than components — libraries \
                     are compositions of system primitives ONLY (TASK-0081 C3)",
                    path.display()
                ));
            }
            let names = parsed
                .decls
                .iter()
                .filter_map(|decl| match decl {
                    crate::ast::Decl::Component(c) => Some(c.name.text.clone()),
                    _ => None,
                })
                .collect();
            shelf.push((
                SourceFile {
                    path: alloc::format!(
                        "dep:{dep}/{}",
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("component.nx")
                    ),
                    source,
                },
                parsed,
                names,
            ));
        }
        if shelf.is_empty() {
            return Err(alloc::format!("dependency `{dep}`: no components under ui/components"));
        }
        // Reference closure over the shelf: a referenced component's own
        // references join the set until nothing new appears (bounded by the
        // shelf size).
        let mut taken = alloc::vec![false; shelf.len()];
        loop {
            let mut grew = false;
            for (i, (_, parsed, names)) in shelf.iter().enumerate() {
                if taken[i] || !names.iter().any(|n| referenced.contains(n)) {
                    continue;
                }
                taken[i] = true;
                grew = true;
                collect_referenced_names(parsed, &mut referenced);
            }
            if !grew {
                break;
            }
        }
        let mut contributed = 0usize;
        for (i, (file, _, _)) in shelf.into_iter().enumerate() {
            if taken[i] {
                files.push(file);
                contributed += 1;
            }
        }
        if contributed == 0 {
            return Err(alloc::format!(
                "dependency `{dep}`: the app references none of its components (drop the dependency)"
            ));
        }
    }
    Ok(())
}

/// Every widget/component NAME a file's pages and components reference
/// (registry widgets included — they are harmless in the set; only library
/// component names are ever looked up in it).
#[cfg(feature = "std")]
fn collect_referenced_names(
    file: &crate::ast::File,
    out: &mut alloc::collections::BTreeSet<String>,
) {
    fn walk(node: &crate::ast::ViewNode, out: &mut alloc::collections::BTreeSet<String>) {
        use crate::ast::ViewNode;
        match node {
            ViewNode::Widget(widget) => {
                out.insert(widget.name.text.clone());
                for child in &widget.children {
                    walk(child, out);
                }
                for binding in &widget.slot_bodies {
                    for child in &binding.body {
                        walk(child, out);
                    }
                }
            }
            ViewNode::If { arms, els, .. } => {
                for (_, body) in arms {
                    for child in body {
                        walk(child, out);
                    }
                }
                for child in els {
                    walk(child, out);
                }
            }
            ViewNode::For { body, .. } => {
                for child in body {
                    walk(child, out);
                }
            }
            ViewNode::Collection(collection) => {
                out.insert(collection.kind.text.clone());
                for child in &collection.body {
                    walk(child, out);
                }
            }
            ViewNode::Match { arms, .. } => {
                for arm in arms {
                    for child in &arm.body {
                        walk(child, out);
                    }
                }
            }
            ViewNode::Slot { .. } => {}
        }
    }
    for decl in &file.decls {
        match decl {
            crate::ast::Decl::Page(page) => walk(&page.view, out),
            crate::ast::Decl::Component(component) => walk(&component.view, out),
            _ => {}
        }
    }
}

/// Reads the app manifest's `dependencies = ["lib", "lib@^1.0", …]` names
/// (the version constraint after `@` is nxb-pack's concern; the build
/// resolver only needs the folder name). Missing manifest = no deps.
#[cfg(feature = "std")]
fn manifest_dependencies(root: &std::path::Path) -> Result<alloc::vec::Vec<String>, String> {
    let manifest = root.join("manifest.toml");
    if !manifest.is_file() {
        return Ok(alloc::vec::Vec::new());
    }
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| alloc::format!("read {}: {e}", manifest.display()))?;
    let mut deps = alloc::vec::Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let starts = line.starts_with("dependencies") && line.contains('=');
        if starts || in_deps {
            in_deps = true;
            let mut chunks = line.split('"');
            while let (Some(_), Some(value)) = (chunks.next(), chunks.next()) {
                let name = value.split('@').next().unwrap_or(value);
                if !name.is_empty() {
                    deps.push(String::from(name));
                }
            }
            if line.ends_with(']') {
                in_deps = false;
            }
        }
    }
    Ok(deps)
}
