// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Host reproduction of the greeter-centering path: compile a Spacer-centered
//! page, mount it, lay it out at a BOUNDED viewport (the app-host contract,
//! `layout_with_viewport`) and assert the card actually lands centered — the
//! width-only layout hugged everything top-left on the real greeter.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir
}

#[test]
fn spacer_centering_works_under_bounded_viewport() {
    let nxir = compile(
        r#"Page Main {
    Stack {
        Spacer
        Card {
            Stack {
                Text("hello")
            }
        }
        Spacer
    }
    .align(center)
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let (w, h) = (1280i32, 800i32);
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(w),
            Some(nexus_layout_types::FxPx::new(h)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    // Find the card box (the tallest non-root box that isn't a spacer): assert
    // its vertical midpoint is near the viewport middle and it is not hugging
    // the top-left corner.
    let mut dump = String::new();
    for b in &layout.boxes {
        dump.push_str(&format!(
            "node={} id={:?} x={} y={} w={} h={}\n",
            b.node_id,
            b.id,
            b.rect.x.as_i32(),
            b.rect.y.as_i32(),
            b.rect.width.as_i32(),
            b.rect.height.as_i32()
        ));
    }
    // The card is the first box below the root whose height is small (content)
    // but y is pushed down by the top spacer.
    let centered = layout.boxes.iter().any(|b| {
        let y = b.rect.y.as_i32();
        let hgt = b.rect.height.as_i32();
        hgt > 0 && hgt < h / 2 && (y + hgt / 2 - h / 2).abs() < h / 8
    });
    assert!(centered, "no vertically centered box found; layout:\n{dump}");
}

/// Reproduction of the greeter user-list tap: `List($state.users)` renders
/// per-item buttons whose `on Tap` must hit-test correctly (the real greeter
/// logged `apphost: input tap miss` on every user-row tap).
#[test]
fn list_item_buttons_are_tappable() {
    struct NoIo;
    impl nexus_dsl_runtime::EffectHost for NoIo {
        fn call(
            &mut self,
            _svc: &str,
            _method: &str,
            _args: &[nexus_dsl_runtime::Value],
            _timeout_ms: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            Err(0)
        }
    }
    let nxir = compile(
        r#"Store S {
    users: List<Str> = ["alice", "bob"],
    selected: Str = "",
}
Event E { Pick(Str), }
reduce E {
    Pick(name) => state.selected = name,
}
Page Main {
    Stack {
        List($state.users) { user in
            Button { label: user }
                .key(user)
            on Tap -> dispatch(Pick(user))
        }
        .direction(row)
        Text($state.selected)
    }
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    // Handlers were registered at mount; find the first Tap handler's box and
    // tap its centre.
    let handlers = view.handlers();
    assert!(!handlers.is_empty(), "list buttons must register Tap handlers");
    let mut dump = String::new();
    for b in &layout.boxes {
        dump.push_str(&format!(
            "node={} x={} y={} w={} h={}\n",
            b.node_id,
            b.rect.x.as_i32(),
            b.rect.y.as_i32(),
            b.rect.width.as_i32(),
            b.rect.height.as_i32()
        ));
    }
    let (box_id, _) = handlers[0].clone();
    let bx = layout
        .boxes
        .iter()
        .find(|b| b.node_id == box_id)
        .unwrap_or_else(|| panic!("handler box {box_id} missing; layout:\n{dump}"));
    assert!(
        bx.rect.width.as_i32() > 0 && bx.rect.height.as_i32() > 0,
        "handler box {box_id} has no area; layout:\n{dump}"
    );
    let cx = bx.rect.x + nexus_layout_types::FxPx::new(bx.rect.width.as_i32() / 2);
    let cy = bx.rect.y + nexus_layout_types::FxPx::new(bx.rect.height.as_i32() / 2);
    let mut host = NoIo;
    let damage = view
        .pointer(&tokens, &device, &locale, &mut host, &layout.boxes, "Tap", cx, cy)
        .expect("pointer");
    assert!(
        damage.is_some(),
        "tap at ({},{}) on handler box {box_id} missed; layout:\n{dump}",
        cx.as_i32(),
        cy.as_i32()
    );
}

/// Full greeter replica: compile the REAL app (`userspace/apps/greeter`),
/// serve `session.users` from a fake host, then drive the real login flow by
/// tapping the layout: user row → Pick, login button → Submit → `login()`
/// reaches the host with the picked user. This is the exact chain that
/// regressed on the device (`apphost: input tap miss` on user rows).
#[test]
fn greeter_login_flow_is_tappable_end_to_end() {
    struct FakeSession {
        login_user: Option<String>,
        calls: Vec<String>,
    }
    impl nexus_dsl_runtime::EffectHost for FakeSession {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            args: &[nexus_dsl_runtime::Value],
            _timeout_ms: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            self.calls.push(format!("{svc}.{method}({args:?})"));
            match (svc, method) {
                ("session", "users") => Ok(Value::List(vec![Value::Str("jenning".into())])),
                ("session", "login") => {
                    if let Some(Value::Str(user)) = args.first() {
                        self.login_user = Some(user.clone());
                        if user.is_empty() {
                            return Err(2); // UNKNOWN_USER, like sessiond
                        }
                    }
                    Ok(Value::Bool(true))
                }
                _ => Err(0),
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/greeter");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("greeter compiles");
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = FakeSession { login_user: None, calls: Vec::new() };
    view.run_initial_effects(&tokens, &device, &locale, &mut host)
        .expect("initial effects (loads users)");

    let engine = nexus_layout::LayoutEngine::new();
    let layout = |view: &View| {
        engine
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(1280),
                Some(nexus_layout_types::FxPx::new(800)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out")
    };
    let mut boxes = layout(&view).boxes;

    let dump = |boxes: &[nexus_layout::LayoutBox], handlers: &[(usize, _)]| {
        let mut out = String::new();
        for b in boxes {
            out.push_str(&format!(
                "node={} x={} y={} w={} h={}\n",
                b.node_id,
                b.rect.x.as_i32(),
                b.rect.y.as_i32(),
                b.rect.width.as_i32(),
                b.rect.height.as_i32()
            ));
        }
        out.push_str(&format!(
            "handler boxes: {:?}\n",
            handlers.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        ));
        out
    };

    // Drive the flow by tapping handler boxes: non-Tap handlers (the secret
    // field's `on Change`) miss by design; every Tap handler must HIT when
    // its box centre is tapped. The redesigned greeter (handoff 07) places
    // the user list BELOW the submit arrow in tree order — walk handlers in
    // REVERSE so Pick(user) lands before Submit (top-down looped on the
    // failed-submit re-emit forever). Rounds stay bounded.
    let mut tap_hits = 0usize;
    'outer: for _round in 0..6 {
        let handler_ids: Vec<usize> =
            view.handlers().iter().rev().map(|(id, _)| *id).collect();
        assert!(
            !handler_ids.is_empty(),
            "greeter registered no handlers:\n{}",
            dump(&boxes, view.handlers())
        );
        for id in handler_ids {
            let Some(b) = boxes.iter().find(|b| b.node_id == id) else {
                panic!("handler box {id} missing from layout:\n{}", dump(&boxes, view.handlers()));
            };
            let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
            let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
            let hit = view
                .pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy)
                .expect("pointer");
            if let Some(damage) = hit {
                tap_hits += 1;
                if host.login_user.as_deref() == Some("jenning") {
                    break 'outer;
                }
                if damage != nexus_dsl_runtime::Damage::None {
                    // Scene re-emitted: ids are stale — re-layout + new round.
                    boxes = layout(&view).boxes;
                    continue 'outer;
                }
            }
        }
    }
    assert!(
        tap_hits >= 2,
        "expected at least the user row + login button to hit (got {tap_hits}):\n{}",
        dump(&boxes, view.handlers())
    );
    assert_eq!(
        host.login_user.as_deref(),
        Some("jenning"),
        "the login flow never reached the host with the picked user; calls: {:?}",
        host.calls
    );
}

/// The shell-topbar case: a STRETCHED nested row must span its slot, so its
/// Spacer pushes the trailing button to the right edge (the hugging nested
/// row collapsed to content width — Apps button at x=102 in a 1280px bar).
#[test]
fn stretched_nested_row_spacer_pushes_trailing_child_right() {
    let nxir = compile(
        r#"Store S { n: Int = 0, }
Event E { Poke, }
reduce E { Poke => state.n = state.n + 1, }
Page Main {
    Stack {
        Stack {
            Text("Product")
            Spacer
            Button { label: "Apps" }
            on Tap -> dispatch(Poke)
        }
        .direction(row)
        Stack {
            Spacer
        }
        .grow(1)
    }
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let mut dump = String::new();
    for b in &layout.boxes {
        dump.push_str(&format!(
            "node={} x={} y={} w={} h={}\n",
            b.node_id,
            b.rect.x.as_i32(),
            b.rect.y.as_i32(),
            b.rect.width.as_i32(),
            b.rect.height.as_i32()
        ));
    }
    // The trailing interactive box (the Apps button) must end near the right
    // edge of the 1280px bar — not hug at ~x=100.
    let (button_box, _) = view.handlers().first().expect("button registers a handler");
    let b = layout
        .boxes
        .iter()
        .find(|bx| bx.node_id == *button_box)
        .unwrap_or_else(|| panic!("button box missing:\n{dump}"));
    let right = b.rect.x.as_i32() + b.rect.width.as_i32();
    assert!(
        right > 1280 - 64,
        "Apps button must sit at the right edge (right={right}):\n{dump}"
    );
}

/// Kit exposure (TASK-0073/0074): every design-system widget the DSL exposes
/// compiles, mounts and lays out — the DSL `Button`/`Badge`/… IS the kit
/// builder (one SSOT), so this is the "our button is really our button" pin.
#[test]
fn design_kit_widgets_mount_through_the_dsl() {
    let nxir = compile(
        r#"Page Main {
    Stack {
        Badge { label: "Neu" }
        Chip { label: "Filter" }
        Avatar { initials: "JS" }
        Checkbox { checked: true, label: "AGB akzeptieren" }
        Slider { value: 40 }
            .label("Lautstärke")
        Spinner
        ProgressBar { value: 64 }
        Toast { message: "Gespeichert" }
        Banner { title: "Status", message: "Synchronisiert" }
        Skeleton
        ListItem { title: "WLAN", subtitle: "Verbunden", showChevron: true }
        Toolbar { title: "Einstellungen" }
        SearchBar { value: "", placeholder: "Suchen" }
    }
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    // Every widget produced real geometry (no zero-sized kit stubs).
    let sized = layout
        .boxes
        .iter()
        .filter(|b| b.rect.width.as_i32() > 0 && b.rect.height.as_i32() > 0)
        .count();
    assert!(sized >= 10, "expected at least 10 sized boxes, got {sized}");
}

/// Shell desktop replica: compile the REAL app (`userspace/apps/desktop-shell`),
/// serve `bundlemgr.enumerate` from a fake host (the root `Refresh` effect),
/// and verify the desktop app GRID: tiles lay out with real geometry, tapping
/// a tile's centre dispatches `Launch` → `svc.ability.launch(id)` reaches the
/// host, and the hover hit-test resolves the same tile (the wash anchor).
#[test]
fn shell_app_grid_tiles_launch_and_hover() {
    struct FakeRegistry {
        id_sym: u32,
        label_sym: u32,
        icon_sym: u32,
        icon_top_sym: u32,
        icon_bottom_sym: u32,
        icon_art_sym: u32,
        launched: Vec<String>,
    }
    impl nexus_dsl_runtime::EffectHost for FakeRegistry {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            args: &[nexus_dsl_runtime::Value],
            _timeout_ms: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            match (svc, method) {
                ("bundlemgr", "enumerate") => {
                    let row = |id: &str, label: &str| {
                        let mut fields = vec![
                            (self.id_sym, Value::Str(id.into())),
                            (self.label_sym, Value::Str(label.into())),
                            (self.icon_sym, Value::Str("star".into())),
                        (self.icon_top_sym, Value::Str("#4ade80".into())),
                        (self.icon_bottom_sym, Value::Str("#15803d".into())),
                            (self.icon_top_sym, Value::Str("#4ade80".into())),
                            (self.icon_bottom_sym, Value::Str("#15803d".into())),
                            (self.icon_art_sym, Value::Str("".into())),
                        ];
                        fields.sort_by_key(|(sym, _)| *sym);
                        Value::Record(fields)
                    };
                    Ok(Value::List(vec![row("counter", "Counter"), row("chat", "Chat")]))
                }
                ("ability", "launch") => {
                    if let Some(Value::Str(id)) = args.first() {
                        self.launched.push(id.clone());
                    }
                    Ok(Value::Bool(true))
                }
                _ => Err(0),
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("desktop-shell compiles");
    let program_symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts runtime").symbols().to_vec();
    let sym = |name: &str| {
        program_symbols.iter().position(|s| s == name).unwrap_or_else(|| {
            panic!("symbol '{name}' missing from the compiled shell")
        }) as u32
    };
    let mut host = FakeRegistry {
        id_sym: sym("id"),
        label_sym: sym("label"),
        icon_sym: sym("icon"),
        icon_top_sym: sym("iconTop"),
        icon_bottom_sym: sym("iconBottom"),
        icon_art_sym: sym("iconArt"),
        launched: Vec::new(),
    };

    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host)
        .expect("initial effects (enumerate)");

    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let boxes = layout.boxes;

    // The grid registered a Tap handler per tile (+ the top-bar Apps button).
    let tap_handlers: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    assert!(
        tap_handlers.len() >= 3,
        "expected >= 3 tap handlers (2 tiles + Apps), got {}",
        tap_handlers.len()
    );

    // Hover pass (pure, no dispatch): every sized interactive box resolves as
    // its own hover anchor — the wash lands on the tile the tap would hit.
    for id in &tap_handlers {
        let Some(b) = boxes.iter().find(|b| b.node_id == *id) else { continue };
        if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
            continue;
        }
        let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
        let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
        assert_eq!(
            view.hover_box_id(&boxes, "Tap", cx, cy),
            Some(*id),
            "hover anchor must match the tap target"
        );
    }

    // Tap the HOME-GRID tiles: the grid sits between the top bar and the
    // dock, so a tap-handler box centred in that band IS a tile (pills are
    // y<60, the dock y>650). No navigation involved — a tile tap dispatches
    // Launch straight from the home page.
    let handler_ids: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    for id in handler_ids {
        let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
        if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
            continue;
        }
        let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
        let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
        if cy.as_i32() < 60 || cy.as_i32() > 650 {
            continue;
        }
        view.pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy)
            .expect("pointer");
        if !host.launched.is_empty() {
            break;
        }
    }
    assert!(
        host.launched.iter().any(|id| id == "counter" || id == "chat"),
        "no tile tap reached svc.ability.launch (launched={:?})",
        host.launched
    );
}

/// Profile plumbing (Launcher-A): the SAME compiled `.nxir` selects different
/// `ui/platform/<profile>/` override arms purely from the device env the host
/// passes at mount — the contract the app-host's `OP_SURFACE_PROFILE` push
/// feeds. Pinned via the real desktop-shell (its LauncherPage has a phone
/// override): after navigating to /launcher, the phone mount shows a
/// DIFFERENT layout structure than the desktop mount.
fn symbols_of(nxir: &[u8]) -> Vec<String> {
    nexus_dsl_runtime::Runtime::mount(nxir).expect("mounts runtime").symbols().to_vec()
}

#[test]
fn platform_override_arms_select_by_device_env() {
    struct NoServices;
    impl nexus_dsl_runtime::EffectHost for NoServices {
        fn call(
            &mut self,
            _svc: &str,
            _method: &str,
            _args: &[nexus_dsl_runtime::Value],
            _timeout_ms: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            Err(0)
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("desktop-shell compiles");
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();

    // For each device env: try every home handler on a FRESH mount until one
    // navigates to the LAUNCHER page — identified by its `on Change` search
    // handler (the only Change trigger in the shell) — then fingerprint it.
    let launcher_fingerprint = |device: &nexus_dsl_runtime::FixtureEnv| -> Vec<(usize, u32)> {
        let engine = nexus_layout::LayoutEngine::new();
        let home_handler_ids: Vec<usize> = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            let view = View::mount(&nxir, &tokens, device, &locale).expect("mounts");
            view.handlers().iter().map(|(id, _)| *id).collect()
        };
        for id in home_handler_ids {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            let mut view = View::mount(&nxir, &tokens, device, &locale).expect("mounts");
            let mut host = NoServices;
            let boxes = engine
                .layout_with_viewport(
                    view.scene(),
                    nexus_layout_types::FxPx::new(1280),
                    Some(nexus_layout_types::FxPx::new(800)),
                    &nexus_text_baked::measure_text::BakedTextMeasure,
                )
                .expect("lays out")
                .boxes;
            let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
            if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
                continue;
            }
            let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
            let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            let _ = view
                .pointer(&tokens, device, &locale, &mut host, &boxes, "Tap", cx, cy)
                .expect("pointer");
            let change_sym = symbols_of(&nxir).iter().position(|s| s == "Change");
            let mut fp: Vec<(usize, u32)> =
                view.handlers().iter().map(|(id, h)| (*id, h.trigger)).collect();
            fp.sort_unstable();
            if let Some(change_sym) = change_sym {
                if fp.iter().any(|(_, t)| *t == change_sym as u32) {
                    return fp; // landed on the launcher page
                }
            }
        }
        panic!("no home handler navigated to the launcher page");
    };

    let desktop = launcher_fingerprint(&nexus_dsl_runtime::FixtureEnv::desktop());
    let phone = launcher_fingerprint(&nexus_dsl_runtime::FixtureEnv::phone("portrait"));
    assert_ne!(
        desktop, phone,
        "the phone override arm must produce a different launcher structure"
    );
}

/// Control Center (Launcher-D): the status pill navigates to /control, and
/// the appearance/mode tiles dispatch REAL `svc.settings.set` calls with the
/// presentation keys (the app-host routes those to windowd → live apply +
/// persist). Pins the whole chain below the IPC boundary.
#[test]
fn control_center_toggles_reach_settings_set() {
    struct SettingsSpy {
        sets: Vec<(String, String)>,
    }
    impl nexus_dsl_runtime::EffectHost for SettingsSpy {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            args: &[nexus_dsl_runtime::Value],
            _timeout_ms: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            match (svc, method) {
                ("settings", "set") => {
                    if let (Some(Value::Str(k)), Some(Value::Str(v))) = (args.first(), args.get(1))
                    {
                        self.sets.push((k.clone(), v.clone()));
                    }
                    Ok(Value::Bool(true))
                }
                ("bundlemgr", "enumerate") => Ok(Value::List(vec![])),
                _ => Err(0),
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("desktop-shell compiles");
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = SettingsSpy { sets: Vec::new() };
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");

    let engine = nexus_layout::LayoutEngine::new();
    let layout = |view: &View| {
        engine
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(1280),
                Some(nexus_layout_types::FxPx::new(800)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out")
            .boxes
    };
    let boxes = layout(&view);

    // The status pill: a top-bar handler (cy < 60) on the RIGHT half.
    let status = view
        .handlers()
        .iter()
        .map(|(id, _)| *id)
        .filter_map(|id| boxes.iter().find(|b| b.node_id == id))
        .find(|b| {
            b.rect.y.as_i32() + b.rect.height.as_i32() / 2 < 60
                && b.rect.x.as_i32() > 640
                && b.rect.width.as_i32() > 0
        })
        .expect("status pill in the top bar");
    let cx = status.rect.x + nexus_layout_types::FxPx::new(status.rect.width.as_i32() / 2);
    let cy = status.rect.y + nexus_layout_types::FxPx::new(status.rect.height.as_i32() / 2);
    view.pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy)
        .expect("pointer")
        .expect("status pill navigates");

    // The Control-Center PANEL is open (top right, an `.overlay()` layer):
    // tap only handlers INSIDE the panel region — the appearance/mode tiles
    // dispatch SetTheme/SetMode → settings.set with presentation keys.
    // (Tapping arbitrary handlers would hit the overlay's backdrop closer —
    // the layer wins every overlap by node-id order — and close the panel.)
    let boxes = layout(&view);
    let handler_ids: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    for id in handler_ids {
        let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
        if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
            continue;
        }
        // Panel region only: the right-anchored 328-wide panel hangs below
        // the top bar (44..~450). Anything else (nav icons bottom right, the
        // grid) would hit the overlay's BACKDROP closer instead — the layer
        // wins every overlap by node-id order — and close the panel mid-loop.
        if b.rect.y.as_i32() < 44
            || b.rect.y.as_i32() > 450
            || b.rect.x.as_i32() < 900
            || b.rect.width.as_i32() > 340
        {
            continue;
        }
        let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
        let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
        let _ = view.pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy);
    }
    assert!(
        host.sets.iter().any(|(k, _)| k == "ui.theme.mode"),
        "no theme set reached the host: {:?}",
        host.sets
    );
    assert!(
        host.sets.iter().any(|(k, _)| k == "ui.shell.mode"),
        "no shell-mode set reached the host: {:?}",
        host.sets
    );
}


#[test]
fn grow_and_size_mods_reach_the_layout_tree() {
    let nxir = compile(
        r#"Page Main {
    Stack {
        Stack { }
        .height(36)
        Stack { }
        .grow(1)
        Stack { }
        .height(56)
    }
    .gap(0)
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let heights: Vec<i32> = layout
        .boxes
        .iter()
        .skip(1) // the page column itself
        .map(|b| b.rect.height.as_i32())
        .collect();
    assert_eq!(
        heights,
        vec![36, 708, 56],
        "topbar 36 / middle grows to 708 / dock 56 (got {heights:?})"
    );
}


/// Drop-down panels (design_handoff_launcher): `SetPanel("control")` opens
/// the Control-Center panel as an `.overlay()` layer — 328 wide, anchored at
/// the RIGHT edge below the top bar; dispatching the same panel again
/// toggles it closed. The overlay layer spans the full viewport (the
/// backdrop tap-catcher must cover everything below the panel too).
#[test]
fn real_shell_control_panel_overlays_top_right() {
    struct NoServices;
    impl nexus_dsl_runtime::EffectHost for NoServices {
        fn call(&mut self, _s: &str, _m: &str, _a: &[nexus_dsl_runtime::Value], _t: u32)
            -> Result<nexus_dsl_runtime::Value, u32> { Err(0) }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = NoServices;
    view.run_initial_effects(&tokens, &device, &locale, &mut host).ok();
    let (e, c) = view.runtime().event_case("PanelEvent", "SetPanel").expect("SetPanel");
    let layout_boxes = |view: &View| {
        nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(1280),
                Some(nexus_layout_types::FxPx::new(800)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out")
            .boxes
    };
    let panel_box = |boxes: &[nexus_layout::LayoutBox]| {
        boxes
            .iter()
            .find(|b| b.rect.width.as_i32() == 340 && b.rect.x.as_i32() > 850)
            .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
    };
    assert!(panel_box(&layout_boxes(&view)).is_none(), "panel closed at mount");
    view.dispatch(&tokens, &device, &locale, &mut host, e, c,
        vec![nexus_dsl_runtime::Value::Str("control".into())]).expect("open");
    let opened = panel_box(&layout_boxes(&view)).expect("control panel visible top-right");
    assert!(opened.1 >= 40, "panel anchored below the top bar (y={})", opened.1);
    // Toggle: the SAME panel id closes it.
    view.dispatch(&tokens, &device, &locale, &mut host, e, c,
        vec![nexus_dsl_runtime::Value::Str("control".into())]).expect("toggle");
    assert!(panel_box(&layout_boxes(&view)).is_none(), "panel toggled closed");
}

/// Mobile-first width classes (design_handoff_launcher): the SAME shell page
/// selects a different dock family per `device.sizeClass`. compact (phone,
/// <640) = full-width dock row + bare nav row; regular (tablet portrait,
/// <1024) = launcher + dock pill centred with the nav row below; wide
/// (landscape) = three floating elements (asserted by the tablet probe
/// below). Each class must mount + lay out with the dock at the bottom.
/// (On-device the app-host derives the class from the REAL surface width —
/// `size_class_for` in app-host `main.rs`; boot proof for compact/regular
/// waits on virtio-gpu display-info plumbing, the guest mode is fixed
/// 1280×800 today.)
#[test]
fn real_shell_selects_compact_and_regular_dock_families() {
    struct NoServices;
    impl nexus_dsl_runtime::EffectHost for NoServices {
        fn call(&mut self, _s: &str, _m: &str, _a: &[nexus_dsl_runtime::Value], _t: u32)
            -> Result<nexus_dsl_runtime::Value, u32> { Err(0) }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    for (size_class, w) in [("compact", 600), ("regular", 900)] {
        let mut device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
        device.size_class = size_class;
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
        let mut host = NoServices;
        view.run_initial_effects(&tokens, &device, &locale, &mut host).ok();
        let layout = nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(w),
                Some(nexus_layout_types::FxPx::new(800)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out");
        // The dock family sits at the BOTTOM at every width class.
        let dock_bottom = layout
            .boxes
            .iter()
            .any(|b| b.rect.y.as_i32() > 650 && b.rect.height.as_i32() > 30);
        assert!(dock_bottom, "{size_class}@{w}: dock family not at the bottom");
    }
}

#[test]
fn real_shell_column_grows_on_tablet() {
    struct NoServices;
    impl nexus_dsl_runtime::EffectHost for NoServices {
        fn call(&mut self, _s: &str, _m: &str, _a: &[nexus_dsl_runtime::Value], _t: u32)
            -> Result<nexus_dsl_runtime::Value, u32> { Err(0) }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = NoServices;
    view.run_initial_effects(&tokens, &device, &locale, &mut host).ok();
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let mut dump = String::new();
    for b in layout.boxes.iter().take(8) {
        dump.push_str(&format!("node={} y={} h={}\n", b.node_id, b.rect.y.as_i32(), b.rect.height.as_i32()));
    }
    // The dock row must sit at the BOTTOM (y > 700).
    let dock_bottom = layout.boxes.iter().any(|b| b.rect.y.as_i32() > 700 && b.rect.height.as_i32() > 40);
    assert!(dock_bottom, "dock not at the bottom; first boxes:\n{dump}");
}


#[test]
fn shell_grid_tiles_lay_out_in_a_row() {
    struct Registry {
        id_sym: u32,
        label_sym: u32,
        icon_sym: u32,
        icon_top_sym: u32,
        icon_bottom_sym: u32,
        icon_art_sym: u32,
    }
    impl nexus_dsl_runtime::EffectHost for Registry {
        fn call(&mut self, svc: &str, method: &str, _a: &[nexus_dsl_runtime::Value], _t: u32)
            -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            if (svc, method) == ("bundlemgr", "enumerate") {
                let row = |id: &str, label: &str| {
                    let mut fields = vec![
                        (self.id_sym, Value::Str(id.into())),
                        (self.label_sym, Value::Str(label.into())),
                        (self.icon_sym, Value::Str("star".into())),
                        (self.icon_top_sym, Value::Str("#4ade80".into())),
                        (self.icon_bottom_sym, Value::Str("#15803d".into())),
                        (self.icon_art_sym, Value::Str("".into())),
                    ];
                    fields.sort_by_key(|(sym, _)| *sym);
                    Value::Record(fields)
                };
                return Ok(Value::List(vec![
                    row("a", "Alpha"), row("b", "Beta"), row("c", "Gamma"), row("d", "Delta"),
                ]));
            }
            Err(0)
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let symbols = symbols_of(&nxir);
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    let mut host =
        Registry {
            id_sym: sym("id"),
            label_sym: sym("label"),
            icon_sym: sym("icon"),
            icon_top_sym: sym("iconTop"),
            icon_bottom_sym: sym("iconBottom"),
            icon_art_sym: sym("iconArt"),
        };
    let device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let engine = nexus_layout::LayoutEngine::new();
    let boxes = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;
    // The 64px tiles in the home-grid band (below topbar, above dock).
    let tiles: Vec<(i32, i32)> = boxes
        .iter()
        .filter(|b| {
            b.rect.width.as_i32() == 64
                && b.rect.height.as_i32() == 64
                && b.rect.y.as_i32() > 40
                && b.rect.y.as_i32() < 700
        })
        .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
        .collect();
    assert!(tiles.len() >= 4, "expected 4 grid tiles, got {tiles:?}");
    let first_y = tiles[0].1;
    assert!(
        tiles.iter().all(|(_, y)| *y == first_y),
        "grid tiles must share one row: {tiles:?}"
    );
}


#[test]
fn widget_modifiers_survive_inside_branch_arms() {
    let nxir = compile(
        r#"Store S { active: Bool = true, }
Event E { X, }
reduce E { X => state.active = state.active, }
Page Main {
    Stack {
        if $state.active {
            Stack {
                Text("a")
                Text("b")
            }
            .direction(row)
        } else {
            Text("off")
        }
    }
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    fn find_row(node: &nexus_layout_types::LayoutNode) -> bool {
        match node {
            nexus_layout_types::LayoutNode::Stack(st, _, children) => {
                (st.direction == nexus_layout_types::Direction::Row && children.len() == 2)
                    || children.iter().any(find_row)
            }
            _ => false,
        }
    }
    assert!(find_row(view.scene()), "the branch-arm Stack lost its .direction(row)");
}


#[test]
fn list_modifiers_survive_inside_branch_arms() {
    let nxir = compile(
        r#"Store S { items: List<Str> = ["a", "b"], active: Bool = true, }
Event E { X, }
reduce E { X => state.active = state.active, }
Page Main {
    Stack {
        if $state.active {
            List($state.items) { it in
                Text(it).key(it)
            }
            .direction(row)
        } else {
            Text("off")
        }
    }
}
"#,
    );
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    fn find_row(node: &nexus_layout_types::LayoutNode) -> bool {
        match node {
            nexus_layout_types::LayoutNode::Stack(st, _, children) => {
                (st.direction == nexus_layout_types::Direction::Row && children.len() == 2)
                    || children.iter().any(find_row)
            }
            _ => false,
        }
    }
    assert!(find_row(view.scene()), "the branch-arm List lost its .direction(row)");
}
