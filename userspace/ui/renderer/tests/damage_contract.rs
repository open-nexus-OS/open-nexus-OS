// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration coverage for TASK-0054 damage overflow semantics.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 renderer damage contract test
//! TEST_SCOPE: Public `Damage` API and bounded coalescing behavior.
//! TEST_SCENARIOS: Overflow past `DamageRectCount` coalesces to full-frame damage.
//! DEPENDENCIES: `ui_renderer`
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use ui_renderer::{Damage, DamageRectCount, Rect, RenderResult, SurfaceHeight, SurfaceWidth};

#[test]
fn damage_overflow_coalesces_to_full_frame() -> RenderResult<()> {
    let width = SurfaceWidth::new(8)?;
    let height = SurfaceHeight::new(8)?;
    let mut damage = Damage::for_frame(width, height, DamageRectCount::new(2)?)?;
    damage.add(Rect::new(0, 0, 1, 1)?)?;
    damage.add(Rect::new(3, 0, 1, 1)?)?;
    damage.add(Rect::new(6, 0, 1, 1)?)?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 8, 8)?]);
    Ok(())
}
