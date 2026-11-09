// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `nx-dsl run`: compile + mount + first-frame summary (optionally after a route, a dispatch and
//! a transcript replay).

use std::process::ExitCode;

pub(crate) fn cmd_run(args: &[String]) -> ExitCode {
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let route = flag("--route");
    let locale_name = flag("--locale");
    let profile = flag("--profile").unwrap_or_else(|| String::from("desktop"));
    let transcript_path = flag("--transcript");
    let dispatch_case = flag("--dispatch");
    let flag_values: Vec<String> =
        ["--route", "--locale", "--profile", "--transcript", "--dispatch"]
            .iter()
            .filter_map(|n| flag(n))
            .collect();
    let files: Vec<String> =
        crate::nx_files(args).into_iter().filter(|f| !flag_values.contains(f)).collect();
    let Some(path) = files.first() else {
        eprintln!("nx-dsl run: no input file");
        return ExitCode::from(2);
    };
    let prepared = match crate::prepare::prepare("run", path, locale_name.as_deref(), &profile) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let device = &prepared.device;
    let catalog_refs: Vec<&nexus_dsl_runtime::Catalog> = prepared.catalogs.iter().collect();
    let locale = nexus_dsl_runtime::LocaleChain::new(&catalog_refs, &prepared.key_names);

    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = match nexus_dsl_runtime::View::mount(&prepared.nxir, &tokens, device, &locale) {
        Ok(view) => view,
        Err(e) => {
            eprintln!("nx-dsl run: view mount failed: {e:?}");
            return ExitCode::from(1);
        }
    };

    if let Some(route) = &route {
        if let Err(e) = view.navigate(&tokens, device, &locale, route) {
            eprintln!("nx-dsl run: navigate `{route}` failed: {e:?}");
            return ExitCode::from(1);
        }
    }

    // Optional single dispatch, replayed against a transcript host.
    let mut transcript_host = match &transcript_path {
        Some(t) => match std::fs::read_to_string(t) {
            Ok(text) => match nexus_dsl_runtime::svc::TranscriptHost::parse(&text) {
                Ok(host) => Some(host),
                Err(line) => {
                    eprintln!("nx-dsl run: `{t}` line {line}: malformed transcript entry");
                    return ExitCode::from(1);
                }
            },
            Err(e) => {
                eprintln!("nx-dsl: cannot read `{t}`: {e}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    if let Some(case) = &dispatch_case {
        let Some((event, case_idx)) = find_case(&view, case) else {
            eprintln!("nx-dsl run: `{case}` is not a declared event case");
            return ExitCode::from(1);
        };
        let result = match transcript_host.as_mut() {
            Some(host) => {
                view.dispatch(&tokens, device, &locale, host, event, case_idx, Vec::new())
            }
            None => {
                let mut noio = nexus_dsl_runtime::NoIo;
                view.dispatch(&tokens, device, &locale, &mut noio, event, case_idx, Vec::new())
            }
        };
        if let Err(e) = result {
            eprintln!("nx-dsl run: dispatch `{case}` failed: {e:?}");
            return ExitCode::from(1);
        }
    }
    if let Some(host) = &transcript_host {
        if !host.is_clean() {
            eprintln!("nx-dsl run: transcript replay had misses: {:?}", host.misses);
            return ExitCode::from(1);
        }
    }

    println!(
        "mounted: hash {}, profile {profile}, {} deps",
        prepared.program_hash,
        view.deps().len()
    );
    for text in scene_texts(view.scene()) {
        println!("text: {text}");
    }
    ExitCode::SUCCESS
}

/// Resolves a bare case name against the mounted program's events.
fn find_case(view: &nexus_dsl_runtime::View<'_>, case: &str) -> Option<(u32, u32)> {
    let runtime = view.runtime();
    let reader = runtime.reader();
    let root = reader.root().ok()?;
    let case_sym = runtime.symbols().iter().position(|s| s == case)? as u32;
    for (e, decl) in root.get_events().ok()?.iter().enumerate() {
        for (c, case_decl) in decl.get_cases().ok()?.iter().enumerate() {
            if case_decl.get_name() == case_sym {
                return Some((e as u32, c as u32));
            }
        }
    }
    None
}

/// Text nodes of the emitted scene, pre-order (the deterministic summary).
fn scene_texts(scene: &nexus_layout_types::LayoutNode) -> Vec<String> {
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
