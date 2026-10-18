// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The mixed pass taken apart (TASK-0215 5c-3 P4a): what costs a pass of mixed draws
//! milliseconds where a pass of one draw costs a quarter of one (P1: 7.5 ms a mixed pass over
//! 1156x256; its geometry word seen after 11–19 ms on the first pass, 4–7 ms on the next). Six
//! passes into the mixed rung's target and area, each stamped three times — the submit returned
//! (the CPU's share: the streams written, the kick), the geometry word seen, the fragment word
//! seen:
//!
//! - `a`: one opaque draw over the bundle's checker — the base line;
//! - `b`: three opaque draws over the checker — the draw count;
//! - `c`: opaque, straight and rounded over the checker — the pipeline changes;
//! - `d`: one straight draw over the colour block in the plane — a texture in the display's
//!   memory, outside the GPU's cache;
//! - `e`: the mixed pass itself (its straight draw over the block);
//! - `f`: the mixed pass again after 200 ms without GPU work — the GPU's idle.
//!
//! Each of `a`–`e` runs twice and the second is reported (warm). The mixed rung reads the
//! pixels; these passes are only timed.

use alloc::format;
use alloc::string::String;

use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::{Draw, Quad, Template};

use super::device_v2::Device;
use super::render_cost::{Pass, Rig};
use super::render_mixed::{
    pass, quad, Watched, AREA, OPAQUE, RADIUS, ROUNDED, STRAIGHT, STRAIGHT_TINT,
};
use super::render_template::Target;

/// The idle before `f`.
const IDLE_NS: u64 = 200_000_000;

/// What the mixed rung lends the measurement.
pub(super) struct Kit<'a> {
    pub opaque: &'a Placed,
    pub straight: &'a Placed,
    pub rounded: &'a Placed,
    /// The bundle's checker (the GPU's cached memory).
    pub checker: u64,
    /// The colour block's descriptor (the plane, outside the GPU's cache).
    pub block: u64,
    pub target: Target,
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

fn draw(pipeline: &Placed, quad: Quad, texture_va: u64) -> Draw<'_> {
    Draw { pipeline, quad, texture_va }
}

/// `submit/geometry/fragment` in µs: the submit's own time, the geometry after it, the fragment
/// after the geometry.
fn stages(p: &Pass) -> String {
    format!("{}/{}/{}", p.submit_us, p.geom_us.saturating_sub(p.submit_us), p.frag_us)
}

fn once(
    dev: &mut Device,
    rig: &Rig<'_>,
    t: &Template<'_>,
    rings: &mut Watched,
    target: Target,
    (name, draws): (&str, &[Draw<'_>]),
) -> Result<Pass, String> {
    pass(dev, rig, t, rings, draws.len(), &|i| draws[i], AREA, target)
        .map_err(|e| format!("{name}: {e}"))
}

/// The six passes; the line's parts in the module's order.
pub(super) fn measure(
    dev: &mut Device,
    rig: &Rig<'_>,
    t: &Template<'_>,
    rings: &mut Watched,
    kit: &Kit<'_>,
) -> Result<String, String> {
    let white = [1.0; 4];
    let (o, s, r) = (
        quad(OPAQUE, white, 0.0),
        quad(STRAIGHT, STRAIGHT_TINT, 0.0),
        quad(ROUNDED, STRAIGHT_TINT, RADIUS),
    );
    let a = [draw(kit.opaque, o, kit.checker)];
    let b = [
        draw(kit.opaque, o, kit.checker),
        draw(kit.opaque, quad(STRAIGHT, white, 0.0), kit.checker),
        draw(kit.opaque, quad(ROUNDED, white, 0.0), kit.checker),
    ];
    let c = [
        draw(kit.opaque, o, kit.checker),
        draw(kit.straight, s, kit.checker),
        draw(kit.rounded, r, kit.checker),
    ];
    let d = [draw(kit.straight, s, kit.block)];
    let e = [
        draw(kit.opaque, o, kit.checker),
        draw(kit.straight, s, kit.block),
        draw(kit.rounded, r, kit.checker),
    ];
    let mut warm = [None, None, None, None, None];
    for (k, case) in [("a", &a[..]), ("b", &b[..]), ("c", &c[..]), ("d", &d[..]), ("e", &e[..])]
        .into_iter()
        .enumerate()
    {
        once(dev, rig, t, rings, kit.target, case)?;
        warm[k] = Some(once(dev, rig, t, rings, kit.target, case)?);
    }
    // Nothing for the GPU to do for a while, then the mixed pass again.
    let idle_from = now_ns();
    dev.wait_until(idle_from + IDLE_NS, |_, _| false);
    let idle_ms = now_ns().saturating_sub(idle_from) / 1_000_000;
    let cold = once(dev, rig, t, rings, kit.target, ("f", &e[..]))?;
    let [Some(pa), Some(pb), Some(pc), Some(pd), Some(pe)] = warm else {
        return Err(String::from("a warm pass went missing"));
    };
    Ok(format!(
        "(a={} b={} c={} d={} e={} f={} irqs={}/{}/{}/{}/{}/{} idle_ms={idle_ms})",
        stages(&pa),
        stages(&pb),
        stages(&pc),
        stages(&pd),
        stages(&pe),
        stages(&cold),
        pa.irqs,
        pb.irqs,
        pc.irqs,
        pd.irqs,
        pe.irqs,
        cold.irqs
    ))
}
