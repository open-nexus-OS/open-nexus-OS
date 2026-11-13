// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the machine's logical CPUs as the kernel runs them (RFC-0118) — the one place
//! that knows which hart a CPU is. CPU 0 is the boot hart whatever its hart id; everything
//! else in the kernel speaks logical CPUs and asks here when it must name a hart to the
//! firmware (a start, an interrupt to another hart) or a context to the interrupt
//! controller. Read once from the device tree before any secondary starts; before that, and
//! on a tree without a readable `/cpus`, the machine is the boot hart alone.
//! Declared at the crate root and free of target code, so the host tests run it against the
//! trees of `nexus-fdt` (the reference board, QEMU `virt`, and a machine of forms neither uses).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (host): the three trees, the interrupt plan for sparse hart
//!   ids, the machine before the tree is read
//! INVARIANTS: a hart id is never an index; the machine is written once and never changes
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use nexus_fdt::{Fdt, Topology};
use nexus_sched::CpuSet;
use spin::Once;

/// The logical CPUs of this machine: at most as many as a [`CpuSet`] names.
pub type Machine = Topology<{ CpuSet::BITS }>;

static MACHINE: Once<Machine> = Once::new();

/// What the tree said about its harts, for the boot log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Read from `/cpus`.
    Tree,
    /// The tree has no readable `/cpus` or does not name the boot hart: the boot hart alone.
    BootHartAlone,
    /// The tree names the boot hart without what every program needs ([`PROGRAMS_NEED`]):
    /// no program of this image can run here.
    BootHartUnfit,
}

/// The extensions every userspace program of this image is built for (ADR-0099: one
/// hard-float target). A hart that does not state them is no logical CPU — a thread may run
/// on any CPU, and would die there on its first float instruction.
pub const PROGRAMS_NEED: [&str; 2] = ["f", "d"];

/// Reads the machine from `tree` with `boot_hart` as CPU 0.
#[must_use]
pub fn read(tree: Option<&[u8]>, boot_hart: u64) -> (Machine, Source) {
    let read = tree
        .and_then(|bytes| Fdt::new(bytes).ok())
        .map(|fdt| Machine::read(&fdt, boot_hart, &PROGRAMS_NEED));
    match read {
        Some(Ok(machine)) => (machine, Source::Tree),
        Some(Err(nexus_fdt::Error::Lacks)) => (Machine::single(boot_hart), Source::BootHartUnfit),
        _ => (Machine::single(boot_hart), Source::BootHartAlone),
    }
}

/// Fixes the machine for this boot. The first call decides; a later one is ignored. Called
/// with paging off (the tree is read at its physical address), so it hands out no reference:
/// an address taken here is not the one the kernel runs at.
pub fn init(tree: Option<&[u8]>, boot_hart: u64) -> Source {
    let (machine, source) = read(tree, boot_hart);
    MACHINE.call_once(|| machine);
    source
}

/// The machine, once the tree was read.
#[must_use]
pub fn machine() -> Option<&'static Machine> {
    MACHINE.get()
}

/// The number of logical CPUs the kernel runs (1 until the tree is read).
#[must_use]
pub fn hart_count() -> usize {
    machine().map_or(1, Machine::count)
}

/// Every logical CPU the kernel runs, as a set.
#[must_use]
pub fn runs() -> CpuSet {
    CpuSet::first(hart_count())
}

/// The hart id of logical CPU `cpu`.
#[must_use]
pub fn hart_of(cpu: usize) -> Option<u64> {
    machine()?.hart_of(cpu)
}

/// The logical CPU that is hart `hart`.
#[must_use]
pub fn cpu_of(hart: u64) -> Option<usize> {
    machine()?.cpu_of(hart)
}

/// The number of clusters (1 until the tree is read and on a tree that names none).
#[must_use]
pub fn clusters() -> usize {
    machine().map_or(1, Machine::clusters)
}

/// The CPUs of cluster `cluster`; `None` beyond the count.
#[must_use]
pub fn cluster_set(cluster: usize) -> Option<CpuSet> {
    match machine() {
        Some(machine) => machine.cluster_bits(cluster).map(CpuSet::from_bits),
        None => (cluster == 0).then_some(CpuSet::first(1)),
    }
}

/// The interrupt controller's context for logical CPU `cpu`'s supervisor external interrupt.
#[must_use]
pub fn plic_context(cpu: usize) -> Option<u32> {
    machine()?.cpu(cpu)?.plic_context
}

/// Answers one topology query of the sched syscall (`nexus_sched::TOPOLOGY_OP`) for
/// `machine` with the CPUs `online`. `None`: an index beyond its count (`EINVAL` at the ABI).
/// The answer is one register and reads no user memory; it carries no authority.
#[must_use]
pub fn answer(
    machine: &Machine,
    online: CpuSet,
    offers: nexus_sched::Offers,
    what: nexus_sched::TopologyQuery,
    index: usize,
) -> Option<usize> {
    use nexus_sched::TopologyQuery as Query;
    match what {
        Query::Offers => (index == 0).then_some(offers.raw()),
        Query::Cpus => Some(machine.count()),
        Query::Online => Some(online.bits() as usize),
        Query::Clusters => Some(machine.clusters()),
        Query::ClusterSet => machine.cluster_bits(index).map(|bits| bits as usize),
        Query::HartOf => machine.hart_of(index).map(|hart| hart as usize),
    }
}

/// The firmware's width of a hart mask: one register.
const MASK_BITS: u64 = usize::BITS as u64;

/// Plans an interrupt to the CPUs of `targets`: calls `send(mask, base)` — the firmware's
/// form, bit `i` of `mask` naming hart `base + i` — as few times as the harts' ids allow.
/// Dense ids take one call; ids further apart than a register is wide take one call per
/// window. A CPU the machine does not run is skipped.
pub fn plan_ipi(machine: &Machine, targets: CpuSet, mut send: impl FnMut(usize, usize)) {
    let hart = |cpu: usize| machine.hart_of(cpu);
    let mut rest = targets.intersection(CpuSet::first(machine.count()));
    while let Some(base) = rest.iter().filter_map(hart).min() {
        let mut mask = 0usize;
        for cpu in rest.iter() {
            let Some(id) = hart(cpu) else { continue };
            if id - base < MASK_BITS {
                mask |= 1 << (id - base);
                rest = rest.without(CpuSet::from_bits(1 << cpu));
            }
        }
        send(mask, base as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &[u8] = include_bytes!("../../../../../libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
    const VIRT: &[u8] = include_bytes!("../../../../../libs/nexus-fdt/tests/goldens/virt.dtb");
    const FORMS: &[u8] =
        include_bytes!("../../../../../libs/nexus-fdt/tests/goldens/cpus-topology.dtb");

    fn sends(machine: &Machine, targets: CpuSet) -> Vec<(usize, usize)> {
        let mut calls = Vec::new();
        plan_ipi(machine, targets, |mask, base| calls.push((mask, base)));
        calls
    }

    #[test]
    fn the_board_and_virt_are_read_with_the_boot_hart_first() {
        let (board, source) = read(Some(BOARD), 3);
        assert_eq!(source, Source::Tree);
        assert_eq!((board.count(), board.clusters()), (8, 2));
        assert_eq!(
            (board.hart_of(0), board.cpu_of(0), board.cpu_of(3)),
            (Some(3), Some(1), Some(0))
        );
        let (virt, _) = read(Some(VIRT), 0);
        assert_eq!((virt.count(), virt.clusters()), (4, 1));
    }

    #[test]
    fn a_machine_without_a_readable_tree_is_the_boot_hart_alone() {
        for tree in [None, Some(&b"not a tree"[..])] {
            let (machine, source) = read(tree, 7);
            assert_eq!(source, Source::BootHartAlone);
            assert_eq!((machine.count(), machine.hart_of(0), machine.clusters()), (1, Some(7), 1));
        }
        // A tree that does not name the hart it was handed over on is not this machine's.
        assert_eq!(read(Some(BOARD), 99).1, Source::BootHartAlone);
    }

    #[test]
    fn dense_hart_ids_take_one_firmware_call() {
        let (board, _) = read(Some(BOARD), 0);
        // CPUs 4..7 are harts 4..7: one call, the mask relative to the lowest.
        assert_eq!(sends(&board, CpuSet::from_bits(0xf0)), [(0xf, 4)]);
        assert_eq!(sends(&board, CpuSet::first(8)), [(0xff, 0)]);
        assert_eq!(sends(&board, CpuSet::EMPTY), []);
    }

    #[test]
    fn the_plan_names_harts_not_cpus() {
        // Boot hart 5 is CPU 0; CPU 1 is hart 0.
        let (board, _) = read(Some(BOARD), 5);
        assert_eq!(sends(&board, CpuSet::from_bits(0b01)), [(1, 5)]);
        assert_eq!(sends(&board, CpuSet::from_bits(0b10)), [(1, 0)]);
        assert_eq!(sends(&board, CpuSet::from_bits(0b11)), [(0b10_0001, 0)]);
    }

    #[test]
    fn sparse_hart_ids_take_one_call_per_window() {
        // Every paged hart of the tree — CPUs: 0 = hart 0x200, 1 = 0x100, 2 = 0x101,
        // 3 = 0x1_0000_0000.
        let forms = Machine::read(&Fdt::new(FORMS).unwrap(), 0x200, &[]).unwrap();
        assert_eq!(
            sends(&forms, CpuSet::first(4)),
            [(0b11, 0x100), (1, 0x200), (1, 0x1_0000_0000)]
        );
        // The machine this kernel runs: hart 0x101 states no floats and is no CPU.
        let (forms, _) = read(Some(FORMS), 0x200);
        assert_eq!(sends(&forms, CpuSet::first(3)), [(1, 0x100), (1, 0x200), (1, 0x1_0000_0000)]);
    }

    #[test]
    fn test_reject_a_boot_hart_that_cannot_run_the_images_programs() {
        // Hart 0x101 is in the tree, enabled and paged, but states neither F nor D.
        let (machine, source) = read(Some(FORMS), 0x101);
        assert_eq!(source, Source::BootHartUnfit);
        assert_eq!(machine.count(), 1);
        // A hart of the same tree that states them is CPU 0 of three.
        let (machine, source) = read(Some(FORMS), 0x100);
        assert_eq!((source, machine.count(), machine.cpu_of(0x101)), (Source::Tree, 3, None));
    }

    #[test]
    fn a_program_reads_the_machine_the_kernel_runs() {
        use nexus_sched::{Placement, Topology};
        // The reader a program uses, fed by the kernel's answers: the board with CPU 6 lost.
        let (board, _) = read(Some(BOARD), 0);
        let online = CpuSet::from_bits(0xbf);
        let seen = Topology::from_queries(|what, index| {
            answer(&board, online, nexus_sched::Offers::NONE, what, index)
        });
        let seen = seen.unwrap();
        assert_eq!((seen.cpus(), seen.clusters(), seen.online()), (8, 2, online));
        assert_eq!(seen.cluster_set(0).map(CpuSet::bits), Some(0x0f));
        assert_eq!(seen.cluster_set(1).map(CpuSet::bits), Some(0xf0));
        assert_eq!(seen.set_for(Placement::Background).bits(), 0xbe);
        // A machine of forms neither reference uses reads by the same rules.
        let (forms, _) = read(Some(FORMS), 0x200);
        let all = CpuSet::first(forms.count());
        let seen = Topology::from_queries(|what, index| {
            answer(&forms, all, nexus_sched::Offers::NONE, what, index)
        })
        .unwrap();
        assert_eq!((seen.cpus(), seen.clusters()), (forms.count(), forms.clusters()));
        // The hart id is a diagnostic: CPU 0 is the boot hart whatever its id.
        assert_eq!(
            answer(&forms, all, nexus_sched::Offers::NONE, nexus_sched::TopologyQuery::HartOf, 0),
            Some(0x200)
        );
    }

    #[test]
    fn test_reject_topology_index_beyond_count() {
        use nexus_sched::TopologyQuery as Query;
        let (virt, _) = read(Some(VIRT), 0);
        let online = CpuSet::first(4);
        assert_eq!(answer(&virt, online, nexus_sched::Offers::NONE, Query::ClusterSet, 1), None);
        assert_eq!(answer(&virt, online, nexus_sched::Offers::NONE, Query::HartOf, 4), None);
        assert_eq!(
            answer(&virt, online, nexus_sched::Offers::NONE, Query::HartOf, usize::MAX),
            None
        );
        assert_eq!(answer(&virt, online, nexus_sched::Offers::NONE, Query::HartOf, 3), Some(3));
        // What the CPUs offer is one word; it has no second one.
        let offers = nexus_sched::Offers::of_machine(|_| true, true, Some(16));
        assert_eq!(answer(&virt, online, offers, Query::Offers, 0), Some(offers.raw()));
        assert_eq!(answer(&virt, online, offers, Query::Offers, 1), None);
    }

    #[test]
    fn test_reject_an_interrupt_to_a_cpu_the_machine_does_not_run() {
        let (virt, _) = read(Some(VIRT), 0);
        // CPU 9 does not exist on four harts: nothing is sent for it.
        assert_eq!(sends(&virt, CpuSet::from_bits(1 << 9)), []);
        assert_eq!(sends(&virt, CpuSet::from_bits(1 << 9 | 0b100)), [(1, 2)]);
    }
}
