// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Whether the glasses written this frame may share passes (TASK-0335 P3). A render costs
//! 0.52 ms fixed on the board's GPU (the vendor stack, 2026-10-21) and a frame with three
//! re-blurred glasses ran ten passes; shared, it runs four: one scene below the highest
//! written glass over every write's reach, one horizontal pass, one vertical pass, the slot.
//! The one scene pass draws the layers below the highest written glass everywhere — so it is
//! right for a lower written glass only where no layer at or above that glass, and no other
//! written glass's own result, falls inside its reach. The rule is pure and host-tested; a
//! frame it refuses runs the chains one by one, as before.

use super::damage::Area;

/// One written glass: its index in the walk, its scene area (its band's reach) and its own
/// rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written {
    pub index: usize,
    pub scene: Area,
    pub rect: Area,
}

/// Whether the writes may share one scene pass that draws the layers below the LOWEST write
/// (`low`) over every write's scene area: for every write, no layer between `low` and `top`
/// (the layer count) that is not itself a write touches its scene area — a layer below the
/// write would be missing from its backdrop, a layer above it wrongly in it — and no other
/// write's rectangle touches it (that write's old rows would be missing). `extent_at` names
/// each layer's screen extent.
pub fn mergeable(
    writes: &[Written],
    low: usize,
    top: usize,
    extent_at: impl Fn(usize) -> Area,
) -> bool {
    let is_write = |i: usize| writes.iter().any(|w| w.index == i);
    writes.iter().all(|w| {
        (low..top).filter(|i| !is_write(*i)).all(|i| !extent_at(i).touches(w.scene))
            && writes.iter().all(|o| o.index == w.index || !o.rect.touches(w.scene))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: Area = Area::Rect { x0: 0, y0: 0, x1: 1920, y1: 40 };
    const DOCK: Area = Area::Rect { x0: 560, y0: 980, x1: 1360, y1: 1080 };
    const WINDOW: Area = Area::Rect { x0: 200, y0: 200, x1: 900, y1: 700 };

    fn scene(rect: Area) -> Area {
        rect.grown(44)
    }

    #[test]
    fn disjoint_glasses_over_a_window_share_the_scene() {
        // Layers: 0 the window, 1 the bar, 2 the dock — both glasses above the window.
        let extents = [WINDOW, BAR, DOCK];
        let writes = [
            Written { index: 1, scene: scene(BAR), rect: BAR },
            Written { index: 2, scene: scene(DOCK), rect: DOCK },
        ];
        assert!(mergeable(&writes, 1, 3, |i| extents[i]));
    }

    #[test]
    fn test_reject_a_layer_between_two_writes_in_the_uppers_reach() {
        // Layers: 0 the window, 1 glass A, 2 a panel in glass B's reach, 3 glass B: the shared
        // scene (below A) would omit the panel from B's backdrop.
        let a = Area::Rect { x0: 100, y0: 100, x1: 300, y1: 200 };
        let panel = Area::Rect { x0: 650, y0: 300, x1: 800, y1: 420 };
        let b = Area::Rect { x0: 600, y0: 400, x1: 900, y1: 600 };
        let extents = [WINDOW, a, panel, b];
        let writes = [
            Written { index: 1, scene: scene(a), rect: a },
            Written { index: 3, scene: scene(b), rect: b },
        ];
        assert!(!mergeable(&writes, 1, 4, |i| extents[i]), "the panel lies in B's reach");
        // Without the panel in reach, the two share the scene.
        let far = Area::Rect { x0: 1500, y0: 900, x1: 1600, y1: 950 };
        let extents = [WINDOW, a, far, b];
        assert!(mergeable(&writes, 1, 4, |i| extents[i]));
    }

    #[test]
    fn test_reject_two_written_glasses_one_over_the_other() {
        // The dock's reach holds a second glass written this frame: the lower one's scene
        // would show the upper's stale rows.
        let pill = Area::Rect { x0: 600, y0: 1000, x1: 700, y1: 1040 };
        let extents = [WINDOW, DOCK, pill];
        let writes = [
            Written { index: 1, scene: scene(DOCK), rect: DOCK },
            Written { index: 2, scene: scene(pill), rect: pill },
        ];
        assert!(!mergeable(&writes, 1, 3, |i| extents[i]));
    }
}
