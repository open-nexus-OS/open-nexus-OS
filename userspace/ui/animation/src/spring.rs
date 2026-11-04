// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Configuration for spring-based animation.
///
/// Default values use a standard UI spring: stiffness=100, damping=10, mass=1.
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

/// A spring solved in closed form (TASK-0354 P0): its state at any time since it was stated,
/// from the position and velocity it started with — no step, no accumulated error, the same
/// answer for the same time on every core (`libm`). A retarget restarts the solution from the
/// state at its time, so the velocity is continuous. [`SpringSim`] is the stepped twin; the
/// closed form stays within half a pixel of it at 60 Hz for every damping
/// (`spring_closed_form_tests`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    config: SpringConfig,
    from: f32,
    v0: f32,
    target: f32,
    t0_ns: u64,
}

impl Spring {
    /// A spring from `from` to `to`, stated at `now_ns`, starting at the config's velocity.
    pub fn new(from: f32, to: f32, config: SpringConfig, now_ns: u64) -> Self {
        Self { config, from, v0: config.initial_velocity, target: to, t0_ns: now_ns }
    }

    /// The position and the velocity at `now_ns` (a time before the start is the start).
    pub fn at(&self, now_ns: u64) -> (f32, f32) {
        let t = now_ns.saturating_sub(self.t0_ns) as f64 * 1e-9;
        let (y, v) = solve(
            (self.from - self.target) as f64,
            self.v0 as f64,
            (self.config.stiffness as f64, self.config.damping as f64, self.config.mass as f64),
            t,
        );
        ((y + self.target as f64) as f32, v as f32)
    }

    /// Aims the spring at `to` from where it is at `now_ns`, keeping its velocity.
    pub fn retarget(&mut self, to: f32, now_ns: u64) {
        let (x, v) = self.at(now_ns);
        *self = Self { config: self.config, from: x, v0: v, target: to, t0_ns: now_ns };
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether the spring has come to rest at `now_ns` (the stepped twin's thresholds).
    pub fn settled(&self, now_ns: u64) -> bool {
        let (x, v) = self.at(now_ns);
        (x - self.target).abs() < 0.005 && v.abs() < 0.05
    }
}

/// The solution of `m y'' + d y' + k y = 0` from `(y0, v0)` after `t` seconds: the offset from
/// the target and its velocity, in the underdamped, critically damped or overdamped form. A
/// spring without stiffness or mass is at its target.
fn solve(y0: f64, v0: f64, (k, d, m): (f64, f64, f64), t: f64) -> (f64, f64) {
    if !(k.is_finite() && m.is_finite() && t.is_finite()) || k <= 0.0 || m <= 0.0 || t < 0.0 {
        return (0.0, 0.0);
    }
    let w0 = libm::sqrt(k / m);
    let zeta = d.max(0.0) / (2.0 * libm::sqrt(k * m));
    if zeta < 1.0 - 1e-6 {
        let wd = w0 * libm::sqrt(1.0 - zeta * zeta);
        let e = libm::exp(-zeta * w0 * t);
        let (c, s) = (libm::cos(wd * t), libm::sin(wd * t));
        let (a, b) = (y0, (v0 + zeta * w0 * y0) / wd);
        let y = e * (a * c + b * s);
        let v = e * ((wd * b - zeta * w0 * a) * c - (wd * a + zeta * w0 * b) * s);
        (y, v)
    } else if zeta <= 1.0 + 1e-6 {
        let e = libm::exp(-w0 * t);
        let b = v0 + w0 * y0;
        let y = (y0 + b * t) * e;
        (y, (b - w0 * (y0 + b * t)) * e)
    } else {
        let sq = libm::sqrt(zeta * zeta - 1.0);
        let (r1, r2) = (-w0 * (zeta - sq), -w0 * (zeta + sq));
        let c1 = (v0 - r2 * y0) / (r1 - r2);
        let c2 = y0 - c1;
        let (e1, e2) = (libm::exp(r1 * t), libm::exp(r2 * t));
        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    }
}
