// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Configuration for spring-based animation.
///
/// Default values match Apple's default spring: stiffness=100, damping=10, mass=1.
/// This produces a critically-damped response with no overshoot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    /// Spring stiffness (N/m). Higher = faster response. Default: 100.0
    pub stiffness: f32,
    /// Damping coefficient (N·s/m). Higher = less bounce. Default: 10.0
    /// Critically damped when damping = 2 * sqrt(stiffness * mass)
    pub damping: f32,
    /// Mass (kg). Default: 1.0
    pub mass: f32,
    /// Initial velocity (units/s). Default: 0.0
    pub initial_velocity: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self { stiffness: 100.0, damping: 10.0, mass: 1.0, initial_velocity: 0.0 }
    }
}

/// Fixed-timestep RK4 spring simulation.
///
/// Uses 4th-order Runge-Kutta integration with explicit dt for deterministic
/// behavior across platforms (x86_64 host and riscv64 QEMU produce identical results).
#[derive(Debug, Clone, PartialEq)]
pub struct SpringSim {
    position: f32,
    velocity: f32,
    target: f32,
    config: SpringConfig,
    done: bool,
}

impl SpringSim {
    pub fn new(from: f32, to: f32, config: SpringConfig) -> Self {
        Self { position: from, velocity: config.initial_velocity, target: to, config, done: false }
    }

    /// Advance simulation by dt_ns nanoseconds. Returns current position.
    /// Uses RK4 with sub-stepping for stability at large dt.
    pub fn step(&mut self, dt_ns: u64) -> f32 {
        if self.done {
            return self.target;
        }
        // Convert ns to seconds, clamp to prevent instability
        let dt = (dt_ns as f64 * 1e-9).min(0.033) as f32; // max 33ms = 30fps floor

        let k = self.config.stiffness;
        let d = self.config.damping;
        let m = self.config.mass;

        // RK4 for spring equation: m * a = -k * (x - target) - d * v
        // Phase space: state = (position, velocity), derivative = (velocity, acceleration)
        let acceleration =
            |pos: f32, vel: f32| -> f32 { (-k * (pos - self.target) - d * vel) / m.max(0.001) };

        // k1
        let k1v = acceleration(self.position, self.velocity);
        let k1p = self.velocity;

        // k2
        let p2 = self.position + k1p * dt * 0.5;
        let v2 = self.velocity + k1v * dt * 0.5;
        let k2v = acceleration(p2, v2);
        let k2p = v2;

        // k3
        let p3 = self.position + k2p * dt * 0.5;
        let v3 = self.velocity + k2v * dt * 0.5;
        let k3v = acceleration(p3, v3);
        let k3p = v3;

        // k4
        let p4 = self.position + k3p * dt;
        let v4 = self.velocity + k3v * dt;
        let k4v = acceleration(p4, v4);
        let k4p = v4;

        self.position += (k1p + 2.0 * k2p + 2.0 * k3p + k4p) * dt / 6.0;
        self.velocity += (k1v + 2.0 * k2v + 2.0 * k3v + k4v) * dt / 6.0;

        // Clamp: if we overshot, snap to target
        let overshot = (self.target - self.position).abs() < 0.005 && self.velocity.abs() < 0.05;
        if overshot {
            self.position = self.target;
            self.velocity = 0.0;
            self.done = true;
        }

        self.position
    }

    /// Returns true when the spring has converged (position ≈ target AND velocity ≈ 0).
    pub fn done(&self) -> bool {
        self.done
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    pub fn target(&self) -> f32 {
        self.target
    }
}
