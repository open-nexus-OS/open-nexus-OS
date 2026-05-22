// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::property::{AnimProp, LayerId, SceneUpdate};
use crate::spring::{SpringConfig, SpringSim};
use crate::keyframe::{Easing, KeyframeTrack};

enum ActiveAnimation {
    Spring { layer: LayerId, prop: AnimProp, sim: SpringSim },
    Keyframe { layer: LayerId, prop: AnimProp, track: KeyframeTrack },
}

pub struct AnimationDriver {
    start: u64,
    last_tick: u64,
    animations: Vec<ActiveAnimation>,
    reduced_motion: bool,
}

impl AnimationDriver {
    pub fn new() -> Self {
        Self { start: 0, last_tick: 0, animations: Vec::new(), reduced_motion: false }
    }

    pub fn tick(&mut self, now_ns: u64) -> Vec<SceneUpdate> {
        if self.start == 0 { self.start = now_ns; }
        let dt = now_ns.saturating_sub(self.last_tick);
        self.last_tick = now_ns;
        if dt == 0 { return Vec::new(); }

        let effective_dt = if self.reduced_motion { dt.min(100_000_000) } else { dt };
        let mut updates = Vec::new();
        let mut i = 0;
        while i < self.animations.len() {
            let done = match &mut self.animations[i] {
                ActiveAnimation::Spring { layer, prop, sim } => {
                    let old = sim.position();
                    let new = sim.step(effective_dt);
                    if (new - old).abs() > 0.0001 {
                        updates.push(SceneUpdate { layer_id: *layer, property: *prop, value: new, progress: if sim.done() { 1.0 } else { 0.0 } });
                    }
                    sim.done()
                }
                ActiveAnimation::Keyframe { layer, prop, track } => {
                    let old = track.value();
                    let new = track.step(effective_dt);
                    if (new - old).abs() > 0.0001 {
                        updates.push(SceneUpdate { layer_id: *layer, property: *prop, value: new, progress: if track.done() { 1.0 } else { 0.0 } });
                    }
                    track.done()
                }
            };
            if done { self.animations.swap_remove(i); } else { i += 1; }
        }
        updates
    }

    pub fn spring_to(&mut self, layer: LayerId, prop: AnimProp, from: f32, target: f32, config: SpringConfig) {
        self.cancel(layer, prop);
        let cfg = if self.reduced_motion { SpringConfig { stiffness: 1000.0, damping: 100.0, ..config } } else { config };
        self.animations.push(ActiveAnimation::Spring { layer, prop, sim: SpringSim::new(from, target, cfg) });
    }

    pub fn keyframe_to(&mut self, layer: LayerId, prop: AnimProp, keyframes: Vec<(f32, f32)>, duration_ns: u64, easing: Easing) {
        self.cancel(layer, prop);
        let dur = if self.reduced_motion { duration_ns.min(100_000_000) } else { duration_ns };
        self.animations.push(ActiveAnimation::Keyframe { layer, prop, track: KeyframeTrack::new(keyframes, dur, easing) });
    }

    pub fn cancel(&mut self, layer: LayerId, prop: AnimProp) {
        self.animations.retain(|a| match a {
            ActiveAnimation::Spring { layer: l, prop: p, .. } => *l != layer || *p != prop,
            ActiveAnimation::Keyframe { layer: l, prop: p, .. } => *l != layer || *p != prop,
        });
    }

    pub fn set_reduced_motion(&mut self, enabled: bool) { self.reduced_motion = enabled; }
    pub fn active_count(&self) -> usize { self.animations.len() }
}

impl Default for AnimationDriver {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spring::SpringSim;

    #[test]
    fn tick_produces_updates() {
        let mut d = AnimationDriver::new();
        d.spring_to(LayerId(1), AnimProp::Opacity, 0.0, 1.0, SpringConfig::default());
        assert!(!d.tick(16_666_667).is_empty());
    }

    #[test]
    fn spring_removed_after_convergence() {
        // Verify SpringSim converges, then verify AnimationDriver removes it
        let mut sim = SpringSim::new(0.0, 1.0, SpringConfig::default());
        for _ in 0..1000 { sim.step(16_666_667); if sim.done() { break; } }
        assert!(sim.done(), "SpringSim must converge");

        // Now test AnimationDriver removal — progressive time
        let mut d = AnimationDriver::new();
        d.spring_to(LayerId(1), AnimProp::Opacity, 0.0, 1.0, SpringConfig { stiffness: 500.0, damping: 40.0, mass: 1.0, initial_velocity: 0.0 });
        let mut t = 16_666_667;
        for _ in 0..2000 {
            t += 16_666_667;
            d.tick(t);
            if d.active_count() == 0 { break; }
        }
        assert_eq!(d.active_count(), 0);
    }

    #[test]
    fn cancel_removes() {
        let mut d = AnimationDriver::new();
        d.spring_to(LayerId(1), AnimProp::Opacity, 0.0, 1.0, SpringConfig::default());
        assert_eq!(d.active_count(), 1);
        d.cancel(LayerId(1), AnimProp::Opacity);
        assert_eq!(d.active_count(), 0);
    }

    #[test]
    fn reduced_motion_caps() {
        let mut d = AnimationDriver::new();
        d.set_reduced_motion(true);
        d.keyframe_to(LayerId(1), AnimProp::Opacity, vec![(0.0, 0.0), (1.0, 1.0)], 1_000_000_000, Easing::Linear);
        assert!(!d.tick(16_666_667).is_empty());
    }
}
