// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::keyframe::{Easing, KeyframeTrack};
use crate::property::{AnimProp, LayerId, SceneUpdate};
use crate::spring::{SpringConfig, SpringSim};
use alloc::vec::Vec;

enum ActiveAnimation {
    Spring { layer: LayerId, prop: AnimProp, sim: SpringSim },
    Keyframe { layer: LayerId, prop: AnimProp, track: KeyframeTrack },
}

const MAX_ACTIVE_ANIMATIONS: usize = 6;

pub struct AnimationDriver {
    start: u64,
    last_tick: u64,
    animations: Vec<ActiveAnimation>,
    reduced_motion: bool,
}

impl AnimationDriver {
    pub fn new() -> Self {
        Self {
            start: 0,
            last_tick: 0,
            animations: Vec::new(),
            reduced_motion: false,
        }
    }

    pub fn tick(&mut self, now_ns: u64) -> Vec<SceneUpdate> {
        let mut updates = Vec::with_capacity(self.animations.len());
        let _ = self.tick_emit(now_ns, |update| updates.push(update));
        updates
    }

    /// Allocation-free tick path for constrained runtimes.
    /// Writes at most `out.len()` updates and returns the number written.
    pub fn tick_into(&mut self, now_ns: u64, out: &mut [SceneUpdate]) -> usize {
        let mut written = 0usize;
        let _ = self.tick_emit(now_ns, |update| {
            if written < out.len() {
                out[written] = update;
                written += 1;
            }
        });
        written
    }

    fn tick_emit<F>(&mut self, now_ns: u64, mut emit: F) -> usize
    where
        F: FnMut(SceneUpdate),
    {
        if self.start == 0 {
            self.start = now_ns;
        }
        let dt = now_ns.saturating_sub(self.last_tick);
        self.last_tick = now_ns;
        if dt == 0 {
            return 0;
        }

        let effective_dt = if self.reduced_motion { dt.min(100_000_000) } else { dt };
        let mut emitted = 0usize;
        let mut i = 0;
        while i < self.animations.len() {
            let done = match &mut self.animations[i] {
                ActiveAnimation::Spring { layer, prop, sim } => {
                    let old = sim.position();
                    let new = sim.step(effective_dt);
                    if (new - old).abs() > 0.0001 {
                        emit(SceneUpdate {
                            layer_id: *layer,
                            property: *prop,
                            value: new,
                            progress: if sim.done() { 1.0 } else { 0.0 },
                        });
                        emitted = emitted.saturating_add(1);
                    }
                    sim.done()
                }
                ActiveAnimation::Keyframe { layer, prop, track } => {
                    let old = track.value();
                    let new = track.step(effective_dt);
                    if (new - old).abs() > 0.0001 {
                        emit(SceneUpdate {
                            layer_id: *layer,
                            property: *prop,
                            value: new,
                            progress: if track.done() { 1.0 } else { 0.0 },
                        });
                        emitted = emitted.saturating_add(1);
                    }
                    track.done()
                }
            };
            if done {
                self.animations.swap_remove(i);
            } else {
                i += 1;
            }
        }
        emitted
    }

    pub fn spring_to(
        &mut self,
        layer: LayerId,
        prop: AnimProp,
        from: f32,
        target: f32,
        config: SpringConfig,
    ) {
        self.cancel(layer, prop);
        let cfg = if self.reduced_motion {
            SpringConfig { stiffness: 1000.0, damping: 100.0, ..config }
        } else {
            config
        };
        self.push_animation(ActiveAnimation::Spring {
            layer,
            prop,
            sim: SpringSim::new(from, target, cfg),
        });
    }

    pub fn keyframe_to(
        &mut self,
        layer: LayerId,
        prop: AnimProp,
        keyframes: Vec<(f32, f32)>,
        duration_ns: u64,
        easing: Easing,
    ) {
        self.cancel(layer, prop);
        let dur = if self.reduced_motion { duration_ns.min(100_000_000) } else { duration_ns };
        self.push_animation(ActiveAnimation::Keyframe {
            layer,
            prop,
            track: KeyframeTrack::new(keyframes, dur, easing),
        });
    }

    pub fn cancel(&mut self, layer: LayerId, prop: AnimProp) {
        self.animations.retain(|a| match a {
            ActiveAnimation::Spring { layer: l, prop: p, .. } => *l != layer || *p != prop,
            ActiveAnimation::Keyframe { layer: l, prop: p, .. } => *l != layer || *p != prop,
        });
    }

    pub fn set_reduced_motion(&mut self, enabled: bool) {
        self.reduced_motion = enabled;
    }
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }
    pub fn active_count(&self) -> usize {
        self.animations.len()
    }

    fn push_animation(&mut self, animation: ActiveAnimation) {
        if self.animations.len() >= MAX_ACTIVE_ANIMATIONS {
            // Keep runtime deterministic and allocation-bounded on os-lite:
            // evict oldest active animation instead of growing the Vec.
            self.animations.swap_remove(0);
        }
        self.animations.push(animation);
    }
}

impl Default for AnimationDriver {
    fn default() -> Self {
        Self::new()
    }
}
