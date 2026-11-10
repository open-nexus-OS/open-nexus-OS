// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The motion tokens sampled at named times (TASK-0353 P5): for every token of
//! `MotionToken::ALL`, its primary and secondary property at 0, 25, 50, 75 and 100 % of the
//! token's duration (the theme's step under the light theme), plus the time the eased curve
//! settles within 0.005 of its end. The compositor's enter / exit / reduced-motion springs get
//! the same treatment over their own settle time. The table is checked in as
//! `motion_samples.golden`, one line per sample: `token time_ms prop value` (3 decimals), so a
//! retuned curve, duration or rest value fails here with the exact line that moved.
//!
//! Regenerate the table (after a deliberate motion change only) with:
//! `UPDATE_MOTION_SAMPLES=1 cargo test -p animation --test motion_samples`

use animation::{AnimProp, MotionToken, Spring, SpringConfig};
use nexus_theme_tokens::{BaseTokens, Tokens};

const GOLDEN: &str = "tests/motion_samples.golden";

fn prop_name(p: AnimProp) -> &'static str {
    match p {
        AnimProp::Opacity => "opacity",
        AnimProp::TranslateX => "translateX",
        AnimProp::TranslateY => "translateY",
        AnimProp::ScaleX => "scaleX",
        AnimProp::ScaleY => "scaleY",
        AnimProp::ShadowRadius => "shadowRadius",
        AnimProp::BlurRadius => "blurRadius",
    }
}

/// The value of `prop` at linear progress `t`: from its absent rest to its present rest along
/// the token's easing, exactly as the compositor's motion layer evaluates it.
fn sample(token: MotionToken, prop: AnimProp, t: f32) -> f32 {
    let (from, to) = (token.resting(prop, false), token.resting(prop, true));
    from + (to - from) * token.easing().ease(t)
}

/// The first millisecond after which the eased progress stays within 0.005 of 1.
fn curve_settle_ms(token: MotionToken, duration_ms: u32) -> u32 {
    let mut settled = duration_ms;
    for ms in (0..=duration_ms).rev() {
        let e = token.easing().ease(ms as f32 / duration_ms as f32);
        if (e - 1.0).abs() > 0.005 {
            break;
        }
        settled = ms;
    }
    settled
}

fn line(out: &mut String, name: &str, ms: u32, prop: &str, v: f32) {
    // `+ 0.0` folds a negative zero into zero so the table does not flap on the sign.
    out.push_str(&format!("{name} {ms} {prop} {:.3}\n", (v * 1000.0).round() / 1000.0 + 0.0));
}

fn table() -> String {
    let mut out = String::new();
    for token in MotionToken::ALL {
        let ms = BaseTokens.motion_ms(token.duration());
        let props = [Some(token.primary_prop()), token.secondary_prop()];
        for prop in props.into_iter().flatten() {
            for pct in [0u32, 25, 50, 75, 100] {
                let at = ms * pct / 100;
                let t = if ms == 0 { 1.0 } else { at as f32 / ms as f32 };
                line(&mut out, token.name(), at, prop_name(prop), sample(token, prop, t));
            }
            if ms > 0 {
                let settle = curve_settle_ms(token, ms);
                let t = settle as f32 / ms as f32;
                line(
                    &mut out,
                    token.name(),
                    settle,
                    &format!("{}@settle", prop_name(prop)),
                    sample(token, prop, t),
                );
            }
        }
    }
    // The compositor's springs: enter, exit, reduced motion — sampled over their settle time.
    let springs = [
        (
            "spring:enter",
            SpringConfig { stiffness: 260.0, damping: 24.0, mass: 1.0, initial_velocity: 0.0 },
        ),
        (
            "spring:exit",
            SpringConfig { stiffness: 380.0, damping: 34.0, mass: 1.0, initial_velocity: 0.0 },
        ),
        (
            "spring:reduced",
            SpringConfig { stiffness: 1000.0, damping: 100.0, mass: 1.0, initial_velocity: 0.0 },
        ),
    ];
    for (name, cfg) in springs {
        let spring = Spring::new(0.0, 1.0, cfg, 0);
        let settle_ms =
            (0..=5000u64).find(|ms| spring.settled(ms * 1_000_000)).unwrap_or(5000) as u32;
        for pct in [0u32, 25, 50, 75, 100] {
            let at = settle_ms * pct / 100;
            let (x, _) = spring.at(at as u64 * 1_000_000);
            line(&mut out, name, at, "position", x);
        }
    }
    out
}

#[test]
fn every_token_matches_the_sampled_table() {
    let got = table();
    let path = format!("{}/{GOLDEN}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_MOTION_SAMPLES").is_some() {
        std::fs::write(&path, &got).expect("write the motion samples table");
        return;
    }
    let want = std::fs::read_to_string(&path).expect("tests/motion_samples.golden is checked in");
    if want != got {
        let first = want
            .lines()
            .zip(got.lines())
            .find(|(w, g)| w != g)
            .map(|(w, g)| format!("golden `{w}` vs now `{g}`"))
            .unwrap_or_else(|| {
                format!("{} golden lines vs {} now", want.lines().count(), got.lines().count())
            });
        panic!("a motion sample moved: {first} (UPDATE_MOTION_SAMPLES=1 regenerates after a deliberate change)");
    }
}

#[test]
fn every_token_is_sampled_at_five_named_times() {
    let got = table();
    for token in MotionToken::ALL {
        let primary = format!("{} ", token.name());
        let n = got
            .lines()
            .filter(|l| l.starts_with(&primary) && l.contains(prop_name(token.primary_prop())))
            .count();
        assert!(n >= 5, "{}: {n} primary samples", token.name());
    }
}
