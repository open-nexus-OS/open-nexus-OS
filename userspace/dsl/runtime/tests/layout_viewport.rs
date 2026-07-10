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
    // its box centre is tapped. Rounds: Login-before-Pick fails honestly
    // (UNKNOWN_USER), Pick then Login succeeds — bounded retries.
    let mut tap_hits = 0usize;
    'outer: for _round in 0..6 {
        let handler_ids: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
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
