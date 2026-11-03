// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the GPU's share of the one pool (ADR-0078, RFC-0100 §2). No memory is set aside for
//! the GPU: what it may hold, and what windowd's surfaces may reserve, are fractions of the RAM
//! the kernel took from the device tree (`mm_stats`), read at boot — never a constant. The
//! ceiling is two thirds of RAM, about what the desktop systems with unified memory give a GPU's
//! working set at these sizes; the surfaces' reservation is an eighth, in whole display planes,
//! at least the fixed layout, at most what the ceiling leaves past the ring. The ledger refuses a
//! GPU buffer past the ceiling. The policy lives here until `memd` owns memory policy (6e).
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below — the board's and QEMU's numbers, the floor and the bound, the
//!   ceiling's refusal (`test_reject_a_buffer_past_the_ceiling`)

/// The GPU's ceiling: this fraction of RAM.
pub(crate) const CEILING_OF_RAM: (u64, u64) = (2, 3);
/// The surfaces' reservation: this fraction of RAM.
pub(crate) const SURFACES_OF_RAM: (u64, u64) = (1, 8);
/// The most display planes a surface object spans: the GPU's window for the object holds it
/// (`backend/gpu/desktop.rs`) and the grant's 32-bit length does. (The kernel's page map of a
/// lazy object was the other bound — four bytes a page, flat — until TASK-0352 P6 made it sparse;
/// TASK-0351 P2 lifts this with the bindings.)
pub(crate) const MAX_SURFACE_PLANES: u32 = 128;

/// What a machine gives the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuShare {
    /// Bytes the GPU may hold in all.
    pub ceiling: u64,
    /// Display planes of the surface object: its whole row space — the wallpaper, the retained
    /// plane, the ring's rows (a hole) and the atlas.
    pub surface_planes: u32,
}

/// The GPU's ceiling on a machine of `ram` bytes.
pub(crate) fn ceiling_of(ram: u64) -> u64 {
    ram.saturating_mul(CEILING_OF_RAM.0) / CEILING_OF_RAM.1
}

/// One owner's budget for surface objects on a machine of `ram` bytes (TASK-0352 P3, ADR-0093):
/// the ceiling less the compositor's surface reservation. One app may hold every window the GPU
/// can hold — but never the compositor's eighth; past its budget the app is refused by name, and
/// the compositor and every other owner keep what they hold.
pub(crate) fn owner_budget_of(ram: u64) -> u64 {
    ceiling_of(ram).saturating_sub(ram.saturating_mul(SURFACES_OF_RAM.0) / SURFACES_OF_RAM.1)
}

impl GpuShare {
    /// The share of a machine of `ram` bytes, for display planes of `plane` bytes, a fixed layout
    /// of `layout_planes` and a scan-out ring of `ring_planes`; `max_planes` bounds the surface
    /// object (the grant's 32-bit length, the GPU's address window for it).
    pub(crate) fn of_ram(
        ram: u64,
        plane: u64,
        (layout_planes, ring_planes, max_planes): (u32, u32, u32),
    ) -> Self {
        let ceiling = ceiling_of(ram);
        if plane == 0 {
            return Self { ceiling, surface_planes: layout_planes };
        }
        let wanted = ram.saturating_mul(SURFACES_OF_RAM.0) / SURFACES_OF_RAM.1 / plane;
        let room = (ceiling / plane).saturating_sub(u64::from(ring_planes));
        let planes = wanted.min(room).min(u64::from(max_planes)).max(u64::from(layout_planes));
        Self { ceiling, surface_planes: u32::try_from(planes).unwrap_or(max_planes) }
    }
}

/// A GPU buffer refused: it would take the GPU past its ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PastCeiling {
    pub want: u64,
    pub held: u64,
    pub ceiling: u64,
}

/// GPU memory held against the ceiling: every buffer the device makes or adopts, by the bytes it
/// spans (a lazy object's reservation counts whole — it may all be drawn).
pub(crate) struct Ledger {
    ceiling: u64,
    held: u64,
}

impl Ledger {
    pub(crate) const fn new(ceiling: u64) -> Self {
        Self { ceiling, held: 0 }
    }

    /// Holds `bytes` more, or refuses with what was asked and held.
    pub(crate) fn hold(&mut self, bytes: u64) -> Result<(), PastCeiling> {
        match self.held.checked_add(bytes).filter(|&h| h <= self.ceiling) {
            Some(held) => {
                self.held = held;
                Ok(())
            }
            None => Err(PastCeiling { want: bytes, held: self.held, ceiling: self.ceiling }),
        }
    }

    /// A buffer gone: its bytes back.
    pub(crate) fn give_back(&mut self, bytes: u64) {
        self.held = self.held.saturating_sub(bytes);
    }

    pub(crate) const fn held(&self) -> u64 {
        self.held
    }

    pub(crate) const fn ceiling(&self) -> u64 {
        self.ceiling
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANE: u64 = 1920 * 1080 * 4;
    const PAGE: u64 = 4096;
    /// (the fixed layout's planes, the ring's, the surface object's bound)
    const BOUNDS: (u32, u32, u32) = (13, 3, MAX_SURFACE_PLANES);

    /// The 4 GiB board (cycle 65's pool): two thirds of it for the GPU, an eighth of it — 64
    /// display planes, the atlas 59 of them — for the surfaces.
    #[test]
    fn the_board_gives_the_gpu_two_thirds_and_its_surfaces_64_planes() {
        let ram = 1_043_815 * PAGE;
        let share = GpuShare::of_ram(ram, PLANE, BOUNDS);
        assert_eq!(share.ceiling, ram * 2 / 3);
        assert_eq!(share.surface_planes, 64);
        assert!(u64::from(share.surface_planes) * PLANE <= share.ceiling);
    }

    /// QEMU's 320 MiB: an eighth is less than the fixed layout, which is the floor — the same
    /// rows as before the split.
    #[test]
    fn a_small_machine_keeps_the_fixed_layout() {
        let share = GpuShare::of_ram(77_193 * PAGE, PLANE, BOUNDS);
        assert_eq!(share.surface_planes, 13);
        assert_eq!(share.ceiling, 77_193 * PAGE * 2 / 3);
    }

    /// A large machine: the surfaces stop at their bound, which the grant's 32-bit length holds.
    #[test]
    fn the_surfaces_stop_at_their_bound() {
        let share = GpuShare::of_ram(64 << 30, PLANE, BOUNDS);
        assert_eq!(share.surface_planes, MAX_SURFACE_PLANES);
        assert!(u64::from(MAX_SURFACE_PLANES) * PLANE <= u64::from(u32::MAX));
    }

    /// An owner's budget is the ceiling less the compositor's eighth (TASK-0352 P3): on the 1 GiB
    /// lane 40 windows of one app (504 MB measured) fit, and the compositor's reservation is
    /// never one app's to take.
    #[test]
    fn an_owner_s_budget_leaves_the_compositor_s_eighth() {
        let ram = 1u64 << 30;
        assert_eq!(owner_budget_of(ram), ram * 2 / 3 - ram / 8);
        assert!(owner_budget_of(ram) > 503_808_000);
        assert_eq!(owner_budget_of(0), 0);
    }

    /// What the GPU holds stays under its ceiling: a buffer that would pass it is refused and
    /// holds nothing; a buffer given back makes room again.
    #[test]
    fn test_reject_a_buffer_past_the_ceiling() {
        let mut ledger = Ledger::new(100);
        ledger.hold(60).unwrap();
        assert_eq!(ledger.hold(41), Err(PastCeiling { want: 41, held: 60, ceiling: 100 }));
        assert_eq!(ledger.held(), 60, "a refusal holds nothing");
        assert_eq!(ledger.hold(u64::MAX).map_err(|e| e.held), Err(60), "no overflow");
        ledger.give_back(60);
        ledger.hold(100).unwrap();
        assert_eq!(ledger.held(), ledger.ceiling());
    }
}
