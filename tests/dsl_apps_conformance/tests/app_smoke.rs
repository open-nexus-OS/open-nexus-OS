// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0357 P3: every DSL app proves itself once over — every route mounts and every event
//! case of every store reduces and runs its effects without a runtime error, on a phone and on
//! the desktop. The payloads are neutral (each declared type's zero, a record field by field;
//! `Runtime::case_probes`) and every service refuses, so the effects take their failure arms:
//! a reducer that cannot take an empty string, a zero or an empty list — or an expression that
//! rejects an omitted prop — fails here, not on the device.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::BaseTokens;

/// Every service refuses: each effect takes its failure arm.
struct Refuse;

impl nexus_dsl_runtime::EffectHost for Refuse {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Err(1)
    }
}

/// The apps (a folder with `ui/pages/`); a library has no routes or events of its own.
fn apps() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../userspace/apps");
    let mut apps: Vec<String> = std::fs::read_dir(root)
        .expect("userspace/apps")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("ui/pages").is_dir())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    apps.sort();
    apps
}

#[test]
fn every_route_mounts_and_every_event_reduces() {
    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    let mut driven = 0usize;
    for app in apps() {
        let nxir = common::compile(&app);
        let symbols = common::program_symbols(&nxir);
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        for (dname, device) in
            [("phone", FixtureEnv::phone("portrait")), ("desktop", FixtureEnv::desktop())]
        {
            let mut view = match View::mount(&nxir, &BaseTokens, &device, &locale) {
                Ok(view) => view,
                Err(e) => {
                    failures.push(format!("{app} {dname}: mount: {e:?}"));
                    continue;
                }
            };
            for route in view.runtime.route_probes() {
                if let Err(e) = view.navigate(&BaseTokens, &device, &locale, &route) {
                    failures.push(format!("{app} {dname}: route {route}: {e:?}"));
                }
                let _ = view.navigate_back(&BaseTokens, &device, &locale);
            }
            for probe in view.runtime.case_probes() {
                if !probe.complete {
                    skipped.push(format!("{app}: {}::{}", probe.event, probe.case));
                    continue;
                }
                driven += 1;
                let (event, case) = probe.ids;
                let payload = probe.payload.clone();
                if let Err(e) =
                    view.dispatch(&BaseTokens, &device, &locale, &mut Refuse, event, case, payload)
                {
                    failures.push(format!(
                        "{app} {dname}: {}::{}({:?}): {e:?}",
                        probe.event, probe.case, probe.payload
                    ));
                }
            }
        }
    }
    // A case whose payload holds a service result type (opaque in the IR until service
    // signatures land) has no neutral value; its reducer is driven by that app's own tests.
    skipped.sort();
    skipped.dedup();
    eprintln!(
        "drove {driven} cases; skipped {} (service-typed payloads): {skipped:?}",
        skipped.len()
    );
    assert!(driven > 200, "the probes drove only {driven} cases");
    assert!(skipped.len() <= 2, "cases without a neutral payload: {skipped:?}");
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
