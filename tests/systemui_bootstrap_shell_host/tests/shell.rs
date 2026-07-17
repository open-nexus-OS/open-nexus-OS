// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0080B host proofs: the bootstrap shell + launcher + greeter compile
//! from their REAL project trees, render per profile, and drive launch/login
//! through transcripted service contracts (byte-exact replay; a miss can
//! never masquerade as success).

use nexus_dsl_runtime::svc::{value_to_text, TranscriptHost};
use nexus_dsl_runtime::{FixtureEnv, Value};
use systemui_bootstrap_shell_host::{app_entry, compile_project, texts, Mounted};

fn enumerate_line(mounted: &Mounted<'_>, query: &str, apps: &[(&str, &str)]) -> String {
    let rows: Vec<String> =
        apps.iter().map(|(id, label)| value_to_text(&app_entry(mounted, id, label))).collect();
    format!("call bundlemgr.enumerate(Str(\"{query}\")) -> Ok(List[{}])", rows.join(","))
}

#[test]
fn shell_page_renders_across_profiles_with_chrome_texts() {
    let nxir = compile_project("desktop-shell");
    for env in
        [FixtureEnv::default(), FixtureEnv::phone("portrait"), FixtureEnv::tablet("landscape")]
    {
        let mounted = Mounted::new(&nxir, env);
        let t = texts(mounted.view.scene());
        // Handoff look: every profile carries the top-bar clock and a
        // battery status text (the app grid + launcher entries are icon
        // tiles, so the chrome texts are the render witnesses).
        assert!(t.contains(&"shell.clock".to_string()), "top-bar clock shown: {t:?}");
        assert!(t.contains(&"shell.battery".to_string()), "battery status shown: {t:?}");
    }
}

#[test]
fn launcher_lists_registry_apps_and_tap_launches_by_id() {
    let nxir = compile_project("desktop-shell");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());

    // Navigate to the launcher (the shell's Apps flow) and load the registry.
    mounted.navigate("/launcher");
    let transcript = format!(
        "# nx-transcript v1\n{}\ncall ability.launch(Str(\"counter\")) -> Ok(Bool(true))\n",
        enumerate_line(
            &mounted,
            "",
            &[("chat", "Chat"), ("counter", "Counter"), ("search", "Search")]
        ),
    );
    let mut host = TranscriptHost::parse(&transcript).expect("transcript parses");
    mounted.dispatch(&mut host, "LauncherEvent", "Refresh", vec![]);
    let t = texts(mounted.view.scene());
    assert!(t.contains(&"Counter".to_string()), "registry labels rendered: {t:?}");
    assert!(t.contains(&"Chat".to_string()));

    // Tap flow: Launch("counter") goes through svc.ability.launch with the
    // AppRecord id — the transcript line above only matches that exact call.
    mounted.dispatch(&mut host, "LauncherEvent", "Launch", vec![Value::Str("counter".into())]);
    assert!(host.is_clean(), "misses: {:?}", host.misses);
    assert_eq!(
        mounted.view.runtime.field("LauncherStore", "launching"),
        Some(&Value::Str("".into())),
        "LaunchDone clears the in-flight id"
    );
}

#[test]
fn launcher_search_refilters_through_the_service() {
    let nxir = compile_project("desktop-shell");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());
    mounted.navigate("/launcher");

    let all = enumerate_line(
        &mounted,
        "",
        &[("chat", "Chat"), ("counter", "Counter"), ("search", "Search")],
    );
    // The query travels WITH the call — filtering is the service's job.
    let mut mounted2 = Mounted::new(&nxir, FixtureEnv::default());
    mounted2.navigate("/launcher");
    let filtered = enumerate_line(&mounted2, "cou", &[("counter", "Counter")]);
    let transcript = format!("# nx-transcript v1\n{all}\n{filtered}\n");
    let mut host = TranscriptHost::parse(&transcript).expect("transcript parses");

    mounted.dispatch(&mut host, "LauncherEvent", "Refresh", vec![]);
    assert!(texts(mounted.view.scene()).contains(&"Chat".to_string()));

    // Type "cou": the binding writes the store; Change dispatches the
    // re-query; only the filtered set remains.
    let (store, path) = {
        let sym = mounted.sym("query");
        (mounted.store_index("LauncherStore"), vec![sym])
    };
    mounted.view.runtime.write_binding(store, &path, Value::Str("cou".into())).expect("writes");
    mounted.dispatch(&mut host, "LauncherEvent", "QueryChanged", vec![]);
    let t = texts(mounted.view.scene());
    assert!(t.contains(&"Counter".to_string()), "filtered set rendered: {t:?}");
    assert!(!t.contains(&"Chat".to_string()), "unmatched apps gone: {t:?}");
    assert!(host.is_clean(), "misses: {:?}", host.misses);
}

#[test]
fn launcher_phone_override_diverges_structurally() {
    let nxir = compile_project("desktop-shell");
    let mut desktop = Mounted::new(&nxir, FixtureEnv::default());
    desktop.navigate("/launcher");
    let mut phone = Mounted::new(&nxir, FixtureEnv::phone("portrait"));
    phone.navigate("/launcher");
    // Same program bytes, same store — different page structure: the
    // desktop override is the windowed panel ("Alle Apps" section header
    // leads, the user-identity footer ends it); the phone override stays
    // the single-column list that still ends with its own Back button (the
    // desktop panel has no Back text at all). Text ORDER is the witness.
    let d = texts(desktop.view.scene());
    let p = texts(phone.view.scene());
    assert_ne!(d, p, "profiles must not collapse to one layout");
    assert_eq!(d.first().map(String::as_str), Some("launcher.allApps"));
    assert_eq!(d.last().map(String::as_str), Some("launcher.userName"));
    assert_eq!(p.last().map(String::as_str), Some("launcher.back"));
    assert!(!d.contains(&"launcher.back".to_string()), "desktop panel has no Back: {d:?}");
    assert!(!p.contains(&"launcher.allApps".to_string()), "phone list has no header: {p:?}");
}

#[test]
fn greeter_login_success_and_failure_drive_the_contract_states() {
    let nxir = compile_project("greeter");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());
    let t = texts(mounted.view.scene());
    // Handoff look: date + big clock top-center, pick-user prompt, the
    // password pill and the idle hint.
    assert!(t.contains(&"greeter.clock".to_string()), "greeter renders: {t:?}");
    assert!(t.contains(&"greeter.pickUser".to_string()), "user prompt shown: {t:?}");
    assert!(t.contains(&"greeter.hint".to_string()), "idle hint shown: {t:?}");

    let transcript = "# nx-transcript v1\n\
        call session.users() -> Ok(List[Str(\"admin\"),Str(\"guest\")])\n\
        call session.login(Str(\"admin\"),Str(\"secret\")) -> Ok(Bool(true))\n\
        call session.login(Str(\"admin\"),Str(\"wrong\")) -> Err(7)\n";
    let mut host = TranscriptHost::parse(transcript).expect("transcript parses");

    // Users load from sessiond's list.
    mounted.dispatch(&mut host, "SessionEvent", "Load", vec![]);
    assert!(texts(mounted.view.scene()).contains(&"admin".to_string()));

    // Pick + type + submit: success returns to idle with the secret CLEARED.
    mounted.dispatch(&mut host, "SessionEvent", "Pick", vec![Value::Str("admin".into())]);
    let secret_path = vec![mounted.sym("secret")];
    mounted
        .view
        .runtime
        .write_binding(0, &secret_path, Value::Str("secret".into()))
        .expect("writes");
    mounted.dispatch(&mut host, "SessionEvent", "Submit", vec![]);
    assert_eq!(mounted.view.runtime.field("SessionStore", "phase"), Some(&Value::Int(0)));
    assert_eq!(mounted.view.runtime.field("SessionStore", "secret"), Some(&Value::Str("".into())));

    // Failure: sessiond says no → phase 2, the failure banner renders, the
    // secret never survives a failed attempt.
    mounted
        .view
        .runtime
        .write_binding(0, &secret_path, Value::Str("wrong".into()))
        .expect("writes");
    mounted.dispatch(&mut host, "SessionEvent", "Submit", vec![]);
    assert_eq!(mounted.view.runtime.field("SessionStore", "phase"), Some(&Value::Int(2)));
    assert_eq!(mounted.view.runtime.field("SessionStore", "lastError"), Some(&Value::Int(7)));
    assert!(texts(mounted.view.scene()).contains(&"greeter.failed".to_string()));
    assert!(host.is_clean(), "misses: {:?}", host.misses);
}

#[test]
fn all_pages_pass_lints_and_a11y_checks() {
    // compile_project asserts has_errors == false (labels on interactive
    // nodes, keys on collections, reducer purity, exhaustiveness) — this
    // test pins that BOTH project trees stay lint-clean.
    let _ = compile_project("desktop-shell");
    let _ = compile_project("greeter");
}

#[test]
fn launcher_grid_reorders_and_inserts_by_key() {
    use nexus_dsl_runtime::NoIo;
    let nxir = compile_project("desktop-shell");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());
    mounted.navigate("/launcher");

    let entries = |mounted: &Mounted<'_>, apps: &[(&str, &str)]| {
        Value::List(apps.iter().map(|(id, label)| app_entry(mounted, id, label)).collect())
    };
    // Initial keyed set.
    let initial = entries(&mounted, &[("chat", "Chat"), ("counter", "Counter")]);
    mounted.dispatch(&mut NoIo, "LauncherEvent", "Loaded", vec![initial]);
    let t = texts(mounted.view.scene());
    let chat = t.iter().position(|s| s == "Chat").expect("chat");
    let counter = t.iter().position(|s| s == "Counter").expect("counter");
    assert!(chat < counter);

    // Reorder + insert: the scene follows the keyed collection order.
    let next = entries(&mounted, &[("search", "Search"), ("counter", "Counter"), ("chat", "Chat")]);
    mounted.dispatch(&mut NoIo, "LauncherEvent", "Loaded", vec![next]);
    let t = texts(mounted.view.scene());
    let search = t.iter().position(|s| s == "Search").expect("search");
    let counter = t.iter().position(|s| s == "Counter").expect("counter");
    let chat = t.iter().position(|s| s == "Chat").expect("chat");
    assert!(search < counter && counter < chat, "keyed order followed: {t:?}");
}
