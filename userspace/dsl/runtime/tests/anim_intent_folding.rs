// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! How a driving expression folds into an [`nexus_dsl_runtime::AnimIntent`] —
//! and why `.animate` and `.effect` must not fold it the same way.
//!
//! `.animate(token, value:)` is a PRESENT/ABSENT binding: the host seeds the
//! node at `MotionToken::resting(prop, value != 0)`, which for every
//! opacity-primary token rests a zero value at opacity 0 — invisible, and
//! cascaded to every box inside it. So a value with no numeric reading must
//! never be answered with 0. It stamps NO intent, and the node paints normally.
//!
//! `.effect(token, trigger:)` only asks "did this change", so a string folds to
//! a hash. It used to fold to a constant 0, which made every string-triggered
//! effect in the system dead code — the calculator's display wiggle among them.

use nexus_dsl_runtime::{AnimKind, FixtureEnv, IdentityLocale, Value, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

const SRC: &str = r#"Store S {
    label: Str = "zero",
    n: Int = 0,
}
Event E {
    Set,
}
reduce E {
    Set => {
        state.label = "one";
        state.n = 1;
    },
}
Page Main {
    Stack {
        Stack { Text($state.label) }
            .animate(snappy, value: $state.label)
        Stack { Text($state.label) }
            .animate(snappy, value: $state.n)
        Stack { Text($state.label) }
            .effect(wiggle, trigger: $state.label)
    }
}
"#;

fn mount() -> (Vec<String>, Vec<u8>) {
    let file = nexus_dsl_core::parse_file(SRC).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, SRC).expect("lowers").nxir;
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    (symbols, nxir)
}

/// A `.animate` whose `value:` has no numeric reading stamps NOTHING — the
/// node keeps its identity transform and paints. Answering 0 on the
/// expression's behalf is what blanked a whole card for a day.
#[test]
fn a_non_numeric_animate_value_stamps_no_intent() {
    let (symbols, nxir) = mount();
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    let animates: Vec<_> =
        view.animations().iter().filter(|(_, i)| i.kind == AnimKind::Animate).collect();
    assert_eq!(
        animates.len(),
        1,
        "only the Int-driven `.animate` may be stamped; the Str-driven one has \
         no present/absent reading and must fail OPEN: {animates:?}"
    );
    assert_eq!(animates[0].1.value, 0, "the Int driver is 0 at rest");
}

/// A `.effect` trigger on a string must actually change when the string does —
/// otherwise the effect never fires and looks like a broken animation system.
#[test]
fn a_string_effect_trigger_changes_with_the_string() {
    let (symbols, nxir) = mount();
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    let effect_value = |v: &View<'_>| {
        v.animations()
            .iter()
            .find(|(_, i)| i.kind == AnimKind::Effect)
            .map(|(_, i)| i.value)
            .expect("the effect intent is stamped")
    };
    let before = effect_value(&view);

    let (e, c) = view.runtime().event_case("E", "Set").expect("Set");
    view.dispatch(&tokens, &device, &locale, &mut host, e, c, vec![]).expect("dispatch");

    let after = effect_value(&view);
    assert_ne!(
        before, after,
        "`zero` -> `one` left the trigger at {before}; a constant trigger never \
         fires, so the effect is dead code"
    );
}

/// The `.animate` presence reading itself: a numeric driver moving 0 -> 1 is
/// what the host diffs to (re)start the motion.
#[test]
fn a_numeric_animate_value_tracks_its_driver() {
    let (symbols, nxir) = mount();
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    let animate_value = |v: &View<'_>| {
        v.animations()
            .iter()
            .find(|(_, i)| i.kind == AnimKind::Animate)
            .map(|(_, i)| i.value)
            .expect("the animate intent is stamped")
    };
    assert_eq!(animate_value(&view), 0, "absent at rest");

    let (e, c) = view.runtime().event_case("E", "Set").expect("Set");
    view.dispatch(&tokens, &device, &locale, &mut host, e, c, vec![]).expect("dispatch");
    assert_eq!(animate_value(&view), 1, "present after the driver moved");
}
