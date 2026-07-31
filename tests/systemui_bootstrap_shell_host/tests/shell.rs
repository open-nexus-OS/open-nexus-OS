// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0080B host proofs: the bootstrap shell + launcher + greeter compile
//! from their REAL project trees, render per profile, and drive launch/login
//! through transcripted service contracts (byte-exact replay; a miss can
//! never masquerade as success).

use nexus_dsl_runtime::svc::{value_to_text, TranscriptHost};
use nexus_dsl_runtime::{FixtureEnv, Value};
use systemui_bootstrap_shell_host::{app_entry, compile_project, texts, Mounted};

/// Open the launcher. It is NOT a route any more (`ui/pages/Routes.nx`): since
/// design_handoff_launcher §9/§10 it is an OVERLAY over the shell home, opened
/// by `PanelStore.panel = "launcher"` so the desktop stays visible behind it.
/// The panel dispatch runs through `NoIo` — it fires no effect, so it must not
/// consume a transcript line.
fn open_launcher(mounted: &mut Mounted<'_>) {
    use nexus_dsl_runtime::NoIo;
    mounted.dispatch(&mut NoIo, "PanelEvent", "SetPanel", vec![Value::Str("launcher".into())]);
}

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
        // RFC-0076: the clock is LIVE state now — the placeholder renders
        // until the host's first ClockEvent tick.
        assert!(t.contains(&"--:--".to_string()), "top-bar clock placeholder shown: {t:?}");
        // The battery percentage is `control.batteryPct` since
        // design_handoff_panels: the bar and the Control Center show ONE
        // charge string, so the shell-only `shell.battery` key retired.
        assert!(t.contains(&"control.batteryPct".to_string()), "battery status shown: {t:?}");
    }
}

#[test]
fn launcher_lists_registry_apps_and_tap_launches_by_id() {
    let nxir = compile_project("desktop-shell");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());

    // Open the launcher overlay (the shell's Apps flow) and load the registry.
    open_launcher(&mut mounted);
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
    open_launcher(&mut mounted);

    let all = enumerate_line(
        &mounted,
        "",
        &[("chat", "Chat"), ("counter", "Counter"), ("search", "Search")],
    );
    // The query travels WITH the call — filtering is the service's job.
    let mut mounted2 = Mounted::new(&nxir, FixtureEnv::default());
    open_launcher(&mut mounted2);
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
    open_launcher(&mut desktop);
    let mut phone = Mounted::new(&nxir, FixtureEnv::phone("portrait"));
    open_launcher(&mut phone);
    // Same program bytes, same store — different structure per profile.
    // Desktop takes `LauncherWindow`: the "Alle Apps" section header LEADS the
    // panel and `LauncherFooter`'s user identity ENDS it. Phone takes
    // `LauncherFullscreen`: its own greeting, no section header and no
    // identity footer. Both now render OVER the shell home (the launcher is an
    // overlay, not a route), so home chrome brackets the scene — the witness is
    // the launcher's own keys and their ORDER, not the scene's first/last text.
    let d = texts(desktop.view.scene());
    let p = texts(phone.view.scene());
    assert_ne!(d, p, "profiles must not collapse to one layout");
    let idx = |t: &[String], key: &str| t.iter().position(|s| s == key);
    let (d_header, d_footer) = (idx(&d, "launcher.allApps"), idx(&d, "launcher.userName"));
    assert!(d_header.is_some(), "desktop panel leads with the section header: {d:?}");
    assert!(d_footer.is_some(), "desktop panel ends with the user identity: {d:?}");
    assert!(d_header < d_footer, "header leads, identity footer ends: {d:?}");
    assert!(!d.contains(&"launcher.greeting".to_string()), "desktop has no greeting: {d:?}");
    assert!(p.contains(&"launcher.greeting".to_string()), "phone list has its greeting: {p:?}");
    assert!(!p.contains(&"launcher.allApps".to_string()), "phone list has no header: {p:?}");
    assert!(!p.contains(&"launcher.userName".to_string()), "phone list has no footer: {p:?}");
}

#[test]
fn greeter_login_success_and_failure_drive_the_contract_states() {
    let nxir = compile_project("greeter");
    let mut mounted = Mounted::new(&nxir, FixtureEnv::default());
    let t = texts(mounted.view.scene());
    // Handoff look (TASK-0305): date + hero clock top-center, the active
    // user's avatar + name, the password pill and the idle hint. There is no
    // "pick a user" prompt any more — `session.active()` pre-selects, and the
    // centre block belongs to whoever `selected` points at.
    // RFC-0076: live clock state — placeholder until the first tick.
    assert!(t.contains(&"--:--".to_string()), "greeter clock placeholder renders: {t:?}");
    assert!(t.contains(&"greeter.hint".to_string()), "idle hint shown: {t:?}");

    // `session.users()` returns RECORDS {id, label}: `login` takes the id, the
    // UI shows the label. Field symbols are IR-table indices, so they come
    // from the COMPILED app — and a record's fields are stored sorted by id.
    let (id_sym, label_sym) = (mounted.sym("id"), mounted.sym("label"));
    let (lo, hi) = if id_sym < label_sym { ("id", "label") } else { ("label", "id") };
    let row = |uid: &str, label: &str| {
        let field = |which: &str| match which {
            "id" => format!("{id_sym}:Str(\"{uid}\")"),
            _ => format!("{label_sym}:Str(\"{label}\")"),
        };
        format!("Record{{{},{}}}", field(lo), field(hi))
    };
    let transcript = format!(
        "# nx-transcript v1\n\
         call session.users() -> Ok(List[{},{}])\n\
         call session.active() -> Ok(Str(\"admin\"))\n\
         call session.login(Str(\"admin\"),Str(\"secret\")) -> Ok(Bool(true))\n\
         call session.login(Str(\"admin\"),Str(\"wrong\")) -> Err(7)\n",
        row("admin", "Admin"),
        row("guest", "Gast"),
    );
    let mut host = TranscriptHost::parse(&transcript).expect("transcript parses");

    // Users load from sessiond's list, and the AUTHORITY pre-selects one.
    mounted.dispatch(&mut host, "SessionEvent", "Load", vec![]);
    let t = texts(mounted.view.scene());
    assert!(t.contains(&"Admin".to_string()), "the DISPLAY NAME renders: {t:?}");
    assert!(!t.contains(&"admin".to_string()), "the raw user id must never render: {t:?}");
    assert_eq!(
        mounted.view.runtime.field("SessionStore", "selected"),
        Some(&Value::Str("admin".into())),
        "session.active() pre-selects; the DSL cannot index a list and must not guess"
    );

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
    open_launcher(&mut mounted);

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
