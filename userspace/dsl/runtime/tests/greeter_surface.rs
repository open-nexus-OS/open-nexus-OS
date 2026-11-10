// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Greeter regressions against the REAL app (`userspace/apps/greeter`):
//! compiled, mounted, laid out at a BOUNDED viewport (the app-host contract,
//! `layout_with_viewport`) and driven through the login flow.
//!
//! Two classes of bug live here, both of which shipped silently once:
//! interaction (a tap that lands on nothing) and geometry (type that renders
//! at a size nobody asked for).

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

/// Full greeter replica: compile the REAL app (`userspace/apps/greeter`),
/// serve `session.users`/`session.active` from a fake host, then drive the
/// real login flow by tapping the layout — submit → `login()` reaches the
/// host with the PRE-SELECTED user id (not its display label). This is the
/// exact chain that regressed on the device (`apphost: input tap miss`), and
/// it is also what catches a full-bleed backdrop swallowing every tap.
#[test]
fn greeter_login_flow_is_tappable_end_to_end() {
    struct FakeSession {
        login_user: Option<String>,
        calls: Vec<String>,
        /// IR symbol ids for the record fields the greeter reads. Only
        /// symbols a page actually references exist in the table, so these
        /// are resolved from the COMPILED app, never guessed.
        id_sym: u32,
        label_sym: u32,
    }
    impl nexus_dsl_runtime::EffectHost for FakeSession {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            args: &[nexus_dsl_runtime::Value],
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            self.calls.push(format!("{svc}.{method}({args:?})"));
            match (svc, method) {
                // RECORD rows {id, label} (TASK-0305): `login` takes the id,
                // the UI shows the label. Field symbols are IR-order indices
                // in the compiled greeter — `id` then `label`, sorted.
                ("session", "users") => {
                    let mut fields = vec![
                        (self.id_sym, Value::Str("jenning".into())),
                        (self.label_sym, Value::Str("Jenning".into())),
                    ];
                    fields.sort_by_key(|(sym, _)| *sym);
                    Ok(Value::List(vec![Value::Record(fields)]))
                }
                // Which user sessiond would log in by default — the greeter
                // pre-selects it instead of guessing "element zero".
                ("session", "active") => Ok(Value::Str("jenning".into())),
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

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/greeter");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("greeter compiles");
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let sym = |name: &str| {
        view.runtime
            .symbols()
            .iter()
            .position(|s| s == name)
            .unwrap_or_else(|| panic!("greeter must reference the `{name}` record field"))
            as u32
    };
    let mut host = FakeSession {
        login_user: None,
        calls: Vec::new(),
        id_sym: sym("id"),
        label_sym: sym("label"),
    };
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

    // `session.active` pre-selects the user, so the switcher row is empty
    // (the design hides it below 2 users) and the flow is: tap submit →
    // login. Every Tap handler must HIT when its box centre is tapped;
    // non-Tap handlers (the secret field's `on Change`) miss by design.
    // Walk in REVERSE so a Pick lands before Submit when more than one user
    // is registered. Rounds stay bounded.
    let mut tap_hits = 0usize;
    'outer: for _round in 0..6 {
        let handler_ids: Vec<usize> = view.handlers().iter().rev().map(|(id, _)| *id).collect();
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
        tap_hits >= 1,
        "the submit button never hit — a full-bleed backdrop out of flow will \
         cover every zone and eat the tap (got {tap_hits}):\n{}",
        dump(&boxes, view.handlers())
    );
    assert_eq!(
        host.login_user.as_deref(),
        Some("jenning"),
        "the login flow never reached the host with the picked user; calls: {:?}",
        host.calls
    );
    // The wire carries the ID; the label is display only. Sending the label
    // is what made every DSL login `UNKNOWN_USER`-denied.
    assert_ne!(
        host.login_user.as_deref(),
        Some("Jenning"),
        "login must send the id, not the label"
    );
}

/// The greeter's LAYOUT contract (TASK-0305 / design artifacts: login).
///
/// Geometry, not pixels: the design's numbers are what make the screen read
/// the way it does, and every one of them depends on a different part of the
/// stack — the hero clock on the baked 120px face, the 44px pill on the field
/// variant, the 180px input on `.grow()` reaching a kit widget. A silent
/// regression in any of those shows up here as a number, not as a vague
/// "looks off".
#[test]
fn greeter_layout_matches_the_design_geometry() {
    struct Host {
        id_sym: u32,
        label_sym: u32,
    }
    impl nexus_dsl_runtime::EffectHost for Host {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            _args: &[nexus_dsl_runtime::Value],
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            use nexus_dsl_runtime::Value;
            match (svc, method) {
                ("session", "users") => {
                    let mut fields = vec![
                        (self.id_sym, Value::Str("jenning".into())),
                        (self.label_sym, Value::Str("Jenning".into())),
                    ];
                    fields.sort_by_key(|(sym, _)| *sym);
                    Ok(Value::List(vec![Value::Record(fields)]))
                }
                ("session", "active") => Ok(Value::Str("jenning".into())),
                _ => Err(0),
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/greeter");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("greeter compiles");
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let sym = |name: &str| {
        view.runtime.symbols().iter().position(|s| s == name).expect("record field symbol") as u32
    };
    let mut host = Host { id_sym: sym("id"), label_sym: sym("label") };
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");

    let engine = nexus_layout::LayoutEngine::new();
    let result = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let boxes = &result.boxes;
    let dump = || -> String {
        boxes.iter().fold(String::new(), |mut acc, b| {
            acc.push_str(&format!(
                "node={} x={} y={} w={} h={}\n",
                b.node_id,
                b.rect.x.as_i32(),
                b.rect.y.as_i32(),
                b.rect.width.as_i32(),
                b.rect.height.as_i32()
            ));
            acc
        })
    };
    let find = |pred: &dyn Fn(i32, i32) -> bool, what: &str| {
        boxes
            .iter()
            .find(|b| pred(b.rect.width.as_i32(), b.rect.height.as_i32()))
            .unwrap_or_else(|| panic!("no box for {what}:\n{}", dump()))
    };

    // The root and the content stack both span the surface: the vignette fade
    // and the tint are BACKGROUNDS, not out-of-flow layers.
    assert_eq!(boxes[0].rect.width.as_i32(), 1280);
    assert_eq!(boxes[0].rect.height.as_i32(), 800);
    assert_eq!(boxes[1].rect.height.as_i32(), 800, "the tinted content stack fills the surface");

    // The clock: the `clock` style on a regular screen (RFC-0116: 128 px on a 128 px line, weight
    // 300 — the design artifacts' display × 2).
    let clock = find(&|w, h| h == 128 && w > 240, "the clock band (128 px line)");
    assert!(
        clock.rect.width.as_i32() > 240,
        "clock digits are wide, got {}",
        clock.rect.width.as_i32()
    );

    // The password pill: 44px tall (design height AND minimum hit target), spanning the 340 px
    // login column (the artifact's form), with the input growing to fill what the submit
    // button leaves.
    let pill = find(&|w, h| h == 44 && w > 200, "the 44px password pill");
    assert_eq!(pill.rect.width.as_i32(), 340, "the field spans the 340 px login column");
    // The input: the lg control height (RFC-0116: 44 — headline 15 on its 20 px line, L19),
    // 340 less the capsule's 16 + 16 insets, the 8 gap and the 30 px submit circle.
    let input = find(&|w, h| w == 340 - 70 && h == 44, "the grown TextField input");
    assert!(input.rect.x > pill.rect.x, "the input sits inside the pill");

    // Avatar 96 (the design artifacts' lock screen) + the three 44px session actions. Its
    // initial is the avatar's own style for the size, `largeTitle` — 30 px on a Latin face. With
    // `display` (64 px, only digits baked) the letter fell back to the 14 px face (board cycle
    // 2026-11-10: "der Buchstabe ist zu klein").
    find(&|w, h| w == 96 && h == 96, "the 96px active-user avatar");
    fn text_px(node: &nexus_layout_types::LayoutNode, text: &str) -> Option<i32> {
        use nexus_layout_types::LayoutNode;
        match node {
            LayoutNode::Text(t, _) if t.content.as_str() == text => {
                Some(t.style.font_size.as_i32())
            }
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                children.iter().find_map(|c| text_px(c, text))
            }
            _ => None,
        }
    }
    assert_eq!(text_px(view.scene(), "J"), Some(30), "the initial is largeTitle, 30 px");
    let actions = boxes
        .iter()
        .filter(|b| b.rect.width.as_i32() == 44 && b.rect.height.as_i32() == 44)
        .count();
    assert_eq!(actions, 3, "sleep / restart / power off:\n{}", dump());
}
