// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The random desktop of `frame_runs.rs` (split out under the module-size ratchet): presents of
//! the kinds the reach oracle sends — layers that open, close, move and swap their order, a
//! pointer's rectangle from above, full presents, windows moved, parked, scaled and faded by
//! their transform, content changes named with their layer's key, with origin 0 and with a key
//! the set does not hold.

use super::{Kind, Rng, World, H, W};
use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::layers::ORIGIN_ABOVE;
use nexus_gfx::frame::Transform;

/// A motion of window `id`, `step` of `of` (`kind`: 0 a slide that returns, 1 a parked
/// offset, 2 a scale-in, 3 a fade).
#[derive(Clone, Copy)]
pub struct Motion {
    id: u32,
    kind: u32,
    step: i32,
    of: i32,
    d: (i32, i32),
    origin: (i16, i16),
}

impl Motion {
    fn transform(&self) -> Transform {
        let (s, t) = (self.step.min(self.of + 1 - self.step), Transform::IDENTITY);
        let (dx, dy) = (self.d.0 as i16, self.d.1 as i16);
        match self.kind {
            0 => Transform { dx: dx * s as i16, dy: dy * s as i16, ..t },
            1 => Transform { dx, dy, ..t },
            2 => Transform { scale: (10_000 - 80 * s) as u16, origin: self.origin, ..t },
            _ => Transform { opacity: (255 - 40 * s) as u8, ..t },
        }
    }
}

/// One random present of `world`; `motions` step and start.
pub fn random_present(world: &mut World, motions: &mut Vec<Motion>, rng: &mut Rng) -> Vec<Command> {
    let (mut layers, mut full, mut damage) = (false, false, Vec::new());
    motions.iter_mut().for_each(|m| m.step += 1);
    let rested: Vec<u32> = motions.iter().filter(|m| m.step > m.of).map(|m| m.id).collect();
    motions.retain(|m| m.step <= m.of);
    let n = world.items.len() as u32;
    let pick = |rng: &mut Rng| 1 + rng.below(n - 1) as usize;
    let moving = |ms: &[Motion], id: u32| id != 0 && ms.iter().any(|m| m.id == id);
    let place = |rng: &mut Rng, w: u32, h: u32| (rng.below(W - w + 1), rng.below(H - h + 1));
    match rng.below(10) {
        0 if n < 9 => {
            let kind = [Kind::Opaque, Kind::Sheer, Kind::Sheer, Kind::Glass][rng.below(4) as usize];
            let (w, h) = (24 + 4 * rng.below(20), 16 + 4 * rng.below(12));
            let (x, y) = place(rng, w, h);
            let id = if rng.below(4) > 0 { world.serial + 100 } else { 0 };
            let mut l = world.layer(kind, (x, y, w, h), id);
            if rng.below(5) == 0 {
                l.key = 0;
            }
            world.items.insert(pick(rng).min(world.items.len()), l);
            layers = true;
        }
        1 if n > 4 => {
            let gone = world.items.remove(pick(rng)).layer_id;
            motions.retain(|m| m.id != gone);
            layers = true;
        }
        2 => {
            let k = pick(rng);
            let l = world.items[k];
            if !moving(motions, l.layer_id) {
                (world.items[k].dst_x, world.items[k].dst_y) = place(rng, l.width, l.height);
                layers = true;
            }
        }
        3 => {
            world.items.swap(pick(rng), pick(rng));
            layers = true;
        }
        4 => {
            let rect = TileRect { x: rng.below(W - 8), y: rng.below(H - 8), width: 8, height: 8 };
            damage.push(Command::Damage { rect, origin: ORIGIN_ABOVE });
        }
        5 => (full, layers) = (true, true),
        _ => {}
    }
    // A window at rest starts to move, if it stays on the display.
    if motions.len() < 2 && rng.below(100) < 45 {
        let l = world.items[1 + rng.below(world.items.len() as u32 - 1) as usize];
        let (kind, of) = (rng.below(4), 3 + rng.below(6) as i32);
        let d = (rng.below(13) as i32 - 6, rng.below(13) as i32 - 6);
        let far = if kind == 0 { of / 2 + 1 } else { 1 };
        let (tx, ty) = (l.dst_x as i32 + d.0 * far, l.dst_y as i32 + d.1 * far);
        let on =
            (0..=(W - l.width) as i32).contains(&tx) && (0..=(H - l.height) as i32).contains(&ty);
        if l.layer_id != 0 && !moving(motions, l.layer_id) && (kind >= 2 || (d != (0, 0) && on)) {
            let origin = ((l.dst_x + l.width / 2) as i16, l.dst_y as i16);
            motions.push(Motion { id: l.layer_id, kind, step: 1, of, d, origin });
        }
    }
    world.moves = motions.iter().map(|m| (m.id, m.transform())).collect();
    world.moves.extend(rested.iter().map(|id| (*id, Transform::IDENTITY)));
    // Content changes, each named where its layer is drawn (never in a layer drawn scaled:
    // its texels are not its pixels).
    for _ in 0..if full { 2 } else { rng.below(4) } {
        let k = rng.below(world.items.len() as u32) as usize;
        let l = world.items[k];
        if motions.iter().any(|m| m.id == l.layer_id && l.layer_id != 0 && m.kind == 2) {
            continue;
        }
        let (lx, ly) = (rng.below(l.width), rng.below(l.height));
        let part =
            (lx, ly, 1 + rng.below((l.width - lx).min(30)), 1 + rng.below((l.height - ly).min(20)));
        let origin = match (l.key, rng.below(10)) {
            (0, _) | (_, 8) => 0,
            (key, 0..=7) => key,
            _ => 0xDEAD_0000 + world.serial,
        };
        damage.push(world.change(k, part, origin));
    }
    world.present(layers, (!full).then_some(&damage[..]))
}
