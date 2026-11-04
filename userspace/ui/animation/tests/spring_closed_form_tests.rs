// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A spring solved in closed form at any time (TASK-0354 P0): within half a pixel of the
//! stepped RK4 twin at 60 Hz for every damping the compositor uses, and within a twentieth of
//! a pixel of RK4 stepped at 1 kHz for every damping (the closed form is what the steps
//! converge to — the stiff reduced-motion spring is 2.7 px off at 60 Hz, RK4's error, not
//! ours); its velocity is continuous across a retarget, and the same time gives the same state.

use animation::{Spring, SpringConfig, SpringSim};

const FRAME_NS: u64 = 16_666_667;

fn config(stiffness: f32, damping: f32) -> SpringConfig {
    SpringConfig { stiffness, damping, mass: 1.0, initial_velocity: 0.0 }
}

/// The dampings in use: the crate's default (underdamped), windowd's enter and exit springs,
/// a critically damped one, the reduced-motion spring (stiff) and a strongly overdamped one.
fn every_damping() -> [(&'static str, SpringConfig); 6] {
    [
        ("default", SpringConfig::default()),
        ("enter", config(260.0, 24.0)),
        ("exit", config(380.0, 34.0)),
        ("critical", config(200.0, 2.0 * 200f32.sqrt())),
        ("reduced", config(1000.0, 100.0)),
        ("overdamped", config(100.0, 40.0)),
    ]
}

/// The most the closed form and RK4 stepped `steps` times a frame differ over four seconds of
/// a 400 px travel, compared at every frame until the stepped twin snaps to its target.
fn worst_from_rk4(cfg: SpringConfig, steps: u32) -> f32 {
    let spring = Spring::new(0.0, 400.0, cfg, 0);
    let mut sim = SpringSim::new(0.0, 400.0, cfg);
    let mut worst = 0.0f32;
    let mut now = 0u64;
    for _ in 0..240 {
        let mut stepped = 0.0;
        for _ in 0..steps {
            now += FRAME_NS / u64::from(steps);
            stepped = sim.step(FRAME_NS / u64::from(steps));
        }
        if sim.done() {
            break;
        }
        let (solved, _) = spring.at(now);
        worst = worst.max((solved - stepped).abs());
    }
    worst
}

#[test]
fn the_closed_form_is_within_half_a_pixel_of_rk4_at_60_hz_for_the_compositors_dampings() {
    for (name, cfg) in every_damping() {
        if name == "reduced" {
            continue;
        }
        let worst = worst_from_rk4(cfg, 1);
        assert!(worst <= 0.5, "{name}: {worst} px from RK4 at 60 Hz");
    }
}

#[test]
fn the_closed_form_is_what_rk4_converges_to_for_every_damping() {
    for (name, cfg) in every_damping() {
        let worst = worst_from_rk4(cfg, 16);
        assert!(worst <= 0.05, "{name}: {worst} px from RK4 at 1 kHz");
        // The strongly overdamped spring's slow root decays at 2.7/s: eight seconds for all.
        let spring = Spring::new(0.0, 400.0, cfg, 0);
        assert!(spring.settled(8_000_000_000), "{name} settles within eight seconds");
    }
}

#[test]
fn a_retarget_keeps_the_position_and_the_velocity() {
    let cfg = SpringConfig::default();
    let mut spring = Spring::new(0.0, 400.0, cfg, 0);
    let t1 = 120_000_000;
    let (x, v) = spring.at(t1);
    assert!(v > 100.0, "the spring is moving at the retarget ({v} px/s)");
    spring.retarget(100.0, t1);
    assert_eq!(spring.at(t1), (x, v), "the state at the retarget's time is unchanged");
    assert_eq!(spring.target(), 100.0);
    // One millisecond on, the velocity has changed by no more than the acceleration allows.
    let (x2, v2) = spring.at(t1 + 1_000_000);
    let bound = (cfg.stiffness * (x - 100.0).abs() + cfg.damping * v.abs()) * 0.001 * 1.5 + 0.01;
    assert!((v2 - v).abs() <= bound, "velocity {v} -> {v2} across one millisecond (bound {bound})");
    assert!((x2 - x).abs() <= v.abs() * 0.001 * 1.5 + 0.01, "position {x} -> {x2}");
    // The stepped twin restarted at the retarget forgets the velocity: that is the defect the
    // closed form removes.
    let restarted = SpringSim::new(x, 100.0, cfg);
    let mut restarted_v = restarted.clone();
    let after = restarted_v.step(1_000_000);
    assert!(((after - x) / 0.001 - v).abs() > 50.0, "RK4 restarted jumps from {v} px/s");
}

#[test]
fn the_same_time_gives_the_same_state_and_an_earlier_time_the_start() {
    let spring = Spring::new(10.0, 200.0, config(260.0, 24.0), 5_000_000_000);
    assert_eq!(spring.at(5_300_000_000), spring.at(5_300_000_000));
    assert_eq!(spring.at(4_000_000_000), (10.0, 0.0));
    assert_eq!(spring.at(5_000_000_000), (10.0, 0.0));
}

#[test]
fn a_spring_rests_at_its_target_once_settled() {
    for (name, cfg) in every_damping() {
        let spring = Spring::new(-300.0, 50.0, cfg, 0);
        for t in [8_000_000_000u64, 60_000_000_000, 3_600_000_000_000] {
            let (x, v) = spring.at(t);
            assert!((x - 50.0).abs() < 0.005 && v.abs() < 0.05, "{name} at {t} ns: {x}, {v}");
            assert!(x.is_finite() && v.is_finite(), "{name} at {t} ns");
        }
    }
}

#[test]
fn a_spring_without_stiffness_or_mass_is_at_its_target() {
    for cfg in [config(0.0, 10.0), SpringConfig { mass: 0.0, ..SpringConfig::default() }] {
        let spring = Spring::new(0.0, 400.0, cfg, 0);
        assert_eq!(spring.at(FRAME_NS), (400.0, 0.0));
        assert!(spring.settled(FRAME_NS));
    }
}

#[test]
fn the_initial_velocity_is_the_configs() {
    let cfg = SpringConfig { initial_velocity: 500.0, ..SpringConfig::default() };
    let spring = Spring::new(0.0, 400.0, cfg, 0);
    assert_eq!(spring.at(0).1, 500.0);
    // Ten milliseconds at 500 px/s, plus the pull of 400 px at stiffness 100 (two pixels).
    let (x, _) = spring.at(10_000_000);
    assert!(x > 6.0 && x < 7.5, "ten milliseconds at 500 px/s: {x}");
}
