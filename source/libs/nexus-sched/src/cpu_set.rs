// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `CpuSet` — which logical CPUs a task may run on (RFC-0118). Bit `n` names logical
//! CPU `n`; CPU 0 is the boot hart, a hart id is never a bit. One definition for the kernel,
//! the ABI and every caller: on the wire it is the one register the sched syscall carries.
//! `BITS` is the one compile-time ceiling of the system's CPU count.
//! A set is not an authority: the rule here ([`CpuSet::admit`]) says which sets a kernel
//! accepts, who may place another task is policy (ADR-0049).
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (wire: one `u64` register)
//! TEST_COVERAGE: `tests` below (the admit rule's three refusals, set algebra, iteration)
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use core::fmt;

/// A set of logical CPUs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct CpuSet(u64);

/// Why a kernel refuses a set (`EINVAL` at the ABI; never clamped silently — a program that
/// names a CPU that is not there has a bug the topology would have prevented).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuSetError {
    /// The set names no CPU.
    Empty,
    /// The set names a CPU the kernel does not run.
    BeyondTheHarts,
    /// The set names no CPU that is online: the task would never run.
    NoOnlineCpu,
}

impl CpuSet {
    /// The width of a set: the most logical CPUs a kernel runs.
    pub const BITS: usize = u64::BITS as usize;

    /// No CPU.
    pub const EMPTY: Self = Self(0);

    /// The set a register carries.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// The register that carries the set.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// CPU `cpu` alone; `None` beyond [`Self::BITS`].
    #[must_use]
    pub const fn single(cpu: usize) -> Option<Self> {
        if cpu < Self::BITS {
            Some(Self(1 << cpu))
        } else {
            None
        }
    }

    /// CPUs `0..count` — every CPU of a kernel that runs `count` (at most [`Self::BITS`]).
    #[must_use]
    pub const fn first(count: usize) -> Self {
        if count >= Self::BITS {
            Self(u64::MAX)
        } else {
            Self((1 << count) - 1)
        }
    }

    /// Whether the set names CPU `cpu`.
    #[must_use]
    pub const fn contains(self, cpu: usize) -> bool {
        cpu < Self::BITS && self.0 & (1 << cpu) != 0
    }

    /// Whether the set names no CPU.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The number of CPUs the set names.
    #[must_use]
    pub const fn count(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The CPUs in either set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The CPUs in both sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The CPUs of this set that `other` does not name.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The set with CPU `cpu` added (unchanged beyond [`Self::BITS`]).
    #[must_use]
    pub const fn with(self, cpu: usize) -> Self {
        match Self::single(cpu) {
            Some(one) => self.union(one),
            None => self,
        }
    }

    /// The lowest CPU the set names.
    #[must_use]
    pub const fn lowest(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.trailing_zeros() as usize)
        }
    }

    /// The CPUs the set names, ascending.
    pub fn iter(self) -> impl Iterator<Item = usize> {
        let mut rest = self.0;
        core::iter::from_fn(move || {
            let cpu = Self(rest).lowest()?;
            rest &= rest - 1;
            Some(cpu)
        })
    }

    /// The rule at the ABI boundary: a set a kernel accepts names at least one CPU, none the
    /// kernel does not run (`runs`), and at least one that is `online`.
    pub const fn admit(self, runs: Self, online: Self) -> Result<Self, CpuSetError> {
        if self.is_empty() {
            return Err(CpuSetError::Empty);
        }
        if !self.without(runs).is_empty() {
            return Err(CpuSetError::BeyondTheHarts);
        }
        if self.intersection(online).is_empty() {
            return Err(CpuSetError::NoOnlineCpu);
        }
        Ok(self)
    }

    /// Where a task of this set runs when `home` is not open to it: `home` when the set names
    /// it and it is online, otherwise the lowest online CPU of the set; `None` when the set
    /// names no online CPU (a set that was admitted always names one while that CPU stays
    /// online).
    #[must_use]
    pub const fn home(self, home: usize, online: Self) -> Option<usize> {
        let open = self.intersection(online);
        if open.contains(home) {
            Some(home)
        } else {
            open.lowest()
        }
    }
}

impl fmt::Debug for CpuSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CpuSet({:#x})", self.0)
    }
}

impl fmt::LowerHex for CpuSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine of eight CPUs with CPU 6 lost at bring-up.
    const RUNS: CpuSet = CpuSet::first(8);
    const ONLINE: CpuSet = CpuSet::from_bits(0b1011_1111);

    #[test]
    fn a_set_of_online_cpus_the_kernel_runs_is_admitted_unchanged() {
        let set = CpuSet::from_bits(0b1010_0001);
        assert_eq!(set.admit(RUNS, ONLINE), Ok(set));
        // One online CPU is enough: the offline one in the set is not a reason to refuse.
        let partly = CpuSet::from_bits(0b0100_0001);
        assert_eq!(partly.admit(RUNS, ONLINE), Ok(partly));
    }

    #[test]
    fn test_reject_empty_cpu_set() {
        assert_eq!(CpuSet::EMPTY.admit(RUNS, ONLINE), Err(CpuSetError::Empty));
    }

    #[test]
    fn test_reject_cpu_set_beyond_the_harts() {
        // CPU 8 on a machine of eight — refused although CPU 0 in the same set is fine.
        let set = CpuSet::from_bits(0b1_0000_0001);
        assert_eq!(set.admit(RUNS, ONLINE), Err(CpuSetError::BeyondTheHarts));
        assert_eq!(
            CpuSet::from_bits(u64::MAX).admit(RUNS, ONLINE),
            Err(CpuSetError::BeyondTheHarts)
        );
    }

    #[test]
    fn test_reject_cpu_set_without_an_online_cpu() {
        let lost = CpuSet::single(6).unwrap();
        assert_eq!(lost.admit(RUNS, ONLINE), Err(CpuSetError::NoOnlineCpu));
    }

    #[test]
    fn the_width_is_the_one_ceiling() {
        assert_eq!(CpuSet::BITS, 64);
        assert_eq!(CpuSet::single(63), Some(CpuSet::from_bits(1 << 63)));
        assert_eq!(CpuSet::single(64), None);
        assert_eq!(CpuSet::first(64).bits(), u64::MAX);
        assert_eq!(CpuSet::first(65).bits(), u64::MAX);
        assert_eq!(CpuSet::first(0), CpuSet::EMPTY);
        assert!(!CpuSet::from_bits(u64::MAX).contains(64));
        assert_eq!(CpuSet::EMPTY.with(64), CpuSet::EMPTY);
    }

    #[test]
    fn set_algebra_and_iteration() {
        let a = CpuSet::from_bits(0b0110);
        let b = CpuSet::from_bits(0b1100);
        assert_eq!(a.union(b).bits(), 0b1110);
        assert_eq!(a.intersection(b).bits(), 0b0100);
        assert_eq!(a.without(b).bits(), 0b0010);
        assert_eq!((a.count(), a.lowest(), CpuSet::EMPTY.lowest()), (2, Some(1), None));
        assert!(a.iter().eq([1, 2]));
        assert!(CpuSet::first(64).iter().eq(0..64));
        assert_eq!(CpuSet::EMPTY.iter().count(), 0);
    }

    #[test]
    fn a_task_runs_at_home_or_on_the_lowest_online_cpu_of_its_set() {
        let set = CpuSet::from_bits(0b1100_0010);
        assert_eq!(set.home(7, ONLINE), Some(7));
        // CPU 6 is offline, CPU 3 is not in the set: the lowest open one.
        assert_eq!(set.home(6, ONLINE), Some(1));
        assert_eq!(set.home(3, ONLINE), Some(1));
        assert_eq!(CpuSet::single(6).unwrap().home(6, ONLINE), None);
    }

    #[test]
    fn it_prints_as_its_register() {
        assert_eq!(format!("{:?}", CpuSet::from_bits(0xf0)), "CpuSet(0xf0)");
        assert_eq!(format!("{:x}", CpuSet::from_bits(0xf0)), "f0");
    }
}
