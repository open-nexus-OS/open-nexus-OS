// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host harness for the TASK-0080B bootstrap shell + greeter DSL
//! apps (`userspace/apps/`): compiles the real project trees the way the
//! CLI project mode does (dir walk → `merge_project` → one canonical
//! `.nxir`), mounts them per device profile, and drives interactions with
//! transcripted service exchanges — the launch/login flows exercise the REAL
//! service contracts (`svc.bundlemgr.enumerate`, `svc.ability.launch`,
//! `svc.session.*`), never mock-only paths.
//! OWNERS: @ui
//! STATUS: Functional (TASK-0080B)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: this crate hosts the coverage (`tests/shell.rs`)

use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::BaseTokens;
use std::path::Path;

/// Compiles a `userspace/apps/<app>` project tree to canonical `.nxir`
/// bytes (the CLI project mode chain: walk `ui/`, merge, check, lower).
///
/// # Panics
/// On any parse/check/lower failure — the shell sources must stay valid.
#[must_use]
pub fn compile_project(app_root: &str) -> Vec<u8> {
    use nexus_dsl_core::{canonical_source_set, merge_project, SourceFile};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../userspace/apps").join(app_root);
    let mut files = Vec::new();
    let mut stack = vec![root.join("ui")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable project dir").flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("nx") {
                files.push(SourceFile {
                    path: p
                        .strip_prefix(&root)
                        .expect("under root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    source: std::fs::read_to_string(&p).expect("readable source"),
                });
            }
        }
    }
    let merged = merge_project(&files).expect("project merges");
    let (model, diags) = nexus_dsl_core::check_file(&merged);
    assert!(!nexus_dsl_core::has_errors(&diags), "shell check errors: {diags:?}");
    let canonical = canonical_source_set(&files);
    nexus_dsl_core::lower_file(&merged, &model, &canonical).expect("shell lowers").nxir
}

/// A mounted shell program under a specific device profile.
pub struct Mounted<'p> {
    pub view: View<'p>,
    pub symbols: Vec<String>,
    pub keys: Vec<u32>,
    pub env: FixtureEnv,
}

impl<'p> Mounted<'p> {
    /// # Panics
    /// If the program does not mount.
    #[must_use]
    pub fn new(nxir: &'p [u8], env: FixtureEnv) -> Self {
        let symbols =
            nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
        let keys: Vec<u32> = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir)
            .expect("reads")
            .root()
            .expect("root")
            .get_i18n_keys()
            .expect("keys")
            .iter()
            .map(|k| k.get_key())
            .collect();
        let view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &BaseTokens, &env, &locale).expect("mounts")
        };
        Self { view, symbols, keys, env }
    }

    /// Dispatches an event case by name through the view (effects run
    /// against `host`).
    ///
    /// # Panics
    /// On unknown event/case or a dispatch error.
    pub fn dispatch(
        &mut self,
        host: &mut dyn nexus_dsl_runtime::EffectHost,
        event: &str,
        case: &str,
        payload: Vec<Value>,
    ) -> Damage {
        let (e, c) = self.view.runtime.event_case(event, case).expect("event exists");
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        self.view
            .dispatch(&BaseTokens, &self.env, &locale, host, e, c, payload)
            .expect("dispatch runs")
    }

    /// Navigates to a route path and re-emits the page.
    ///
    /// # Panics
    /// On an unmatched route.
    pub fn navigate(&mut self, path: &str) {
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        self.view.navigate(&BaseTokens, &self.env, &locale, path).expect("route matches");
    }

    /// The interned symbol id of `name`.
    ///
    /// # Panics
    /// If the program never interned it.
    #[must_use]
    pub fn sym(&self, name: &str) -> u32 {
        self.symbols.iter().position(|s| s == name).unwrap_or_else(|| panic!("symbol {name}"))
            as u32
    }
}

/// Collects every text run in the scene (structural snapshot assertions).
#[must_use]
pub fn texts(scene: &nexus_layout_types::LayoutNode) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        use nexus_layout_types::LayoutNode as N;
        match node {
            N::Text(text, _) => out.push(String::from(text.content.as_str())),
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(scene, &mut out);
    out
}

/// An `AppEntry` record value (`{id, label}`) with the program's interned
/// field symbols — what `svc.bundlemgr.enumerate` returns.
#[must_use]
pub fn app_entry(mounted: &Mounted<'_>, id: &str, label: &str) -> Value {
    let id_sym = mounted.sym("id");
    let label_sym = mounted.sym("label");
    let mut fields =
        vec![(id_sym, Value::Str(id.into())), (label_sym, Value::Str(label.into()))];
    fields.sort_by_key(|(sym, _)| *sym);
    Value::Record(fields)
}
