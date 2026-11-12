// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a device line's interrupt is enabled (RFC-0118 "Interrupts", TASK-0330 P8) —
//! the rules, pure: a line is enabled in at most one interrupt-controller context, the one of
//! the CPU its driver thread is homed on; it follows the thread; a claim is completed in the
//! context that claimed it and while the line is still enabled there (a controller ignores the
//! completion of a line that is not enabled for the target — the line would never fire again);
//! a CPU the tree names no context for is never chosen. `crate::irq` keeps one [`Route`] per
//! line under its lock and carries the [`Step`]s out at the controller.
//! OWNERS: @kernel-hal-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host unit tests below (the rules, the rejects, 10 000 random events)
//! INVARIANTS: after every event the line is enabled in at most one context; a context with an
//!   outstanding claim keeps the line enabled until that claim is completed; every claim is
//!   completed exactly once, where it was made; no step names a CPU [`target`] did not allow.

/// A logical CPU (dense from 0, below `CpuSet::BITS`).
pub type Cpu = u8;

/// One instruction to the interrupt controller for one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Enable the line in this CPU's context.
    Enable(Cpu),
    /// Disable the line in this CPU's context.
    Disable(Cpu),
    /// Complete the line's claim in this CPU's context.
    Complete(Cpu),
}

/// What one event asks of the controller, in order (at most three steps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Steps {
    steps: [Option<Step>; 3],
}

impl Steps {
    fn push(&mut self, step: Step) {
        if let Some(slot) = self.steps.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(step);
        }
    }

    /// The steps in the order they are carried out.
    pub fn iter(&self) -> impl Iterator<Item = Step> + '_ {
        self.steps.iter().flatten().copied()
    }
}

/// The CPU that takes a line whose driver is homed on `home`: `home` when it is online and
/// has a context, otherwise the lowest online CPU that has one — CPU 0 on every machine whose
/// boot hart takes external interrupts. `None`: no online CPU has a context; the line stays
/// disabled and is named by the caller.
#[must_use]
pub fn target(home: Cpu, online: u64, has_context: impl Fn(Cpu) -> bool) -> Option<Cpu> {
    let usable = |cpu: Cpu| cpu < 64 && online & (1u64 << cpu) != 0 && has_context(cpu);
    if usable(home) {
        return Some(home);
    }
    (0..64).find(|cpu| usable(*cpu))
}

/// One line's place at the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Route {
    /// The one context the line is enabled in.
    enabled_on: Option<Cpu>,
    /// The context that claimed the line and has not completed it.
    claimed_on: Option<Cpu>,
    /// Where the line belongs (its driver's home); differs from `enabled_on` only while a
    /// claim is outstanding.
    wanted: Option<Cpu>,
}

impl Route {
    /// A line nobody bound: enabled nowhere.
    pub const UNBOUND: Self = Self { enabled_on: None, claimed_on: None, wanted: None };

    /// The context the line is enabled in.
    #[must_use]
    pub const fn enabled_on(&self) -> Option<Cpu> {
        self.enabled_on
    }

    /// The context holding an uncompleted claim.
    #[must_use]
    pub const fn claimed_on(&self) -> Option<Cpu> {
        self.claimed_on
    }

    /// The line belongs on `cpu` now (a bind, or its driver's home changed; `None`: nowhere).
    /// With a claim outstanding nothing moves — the completion moves it.
    #[must_use]
    pub fn want(&mut self, cpu: Option<Cpu>) -> Steps {
        self.wanted = cpu;
        let mut steps = Steps::default();
        if self.claimed_on.is_none() {
            self.settle(&mut steps);
        }
        steps
    }

    /// `cpu`'s context claimed the line (the controller masks it until the completion).
    pub fn claimed(&mut self, cpu: Cpu) {
        self.claimed_on = Some(cpu);
    }

    /// The driver served the device: the claim is completed where it was made, then a move
    /// that waited for it is carried out. Without a claim the completion goes to the context
    /// the line is enabled in (a driver re-arming a line it never saw fire).
    #[must_use]
    pub fn complete(&mut self) -> Steps {
        let mut steps = Steps::default();
        if let Some(cpu) = self.claimed_on.take().or(self.enabled_on) {
            steps.push(Step::Complete(cpu));
        }
        self.settle(&mut steps);
        steps
    }

    /// A line without a driver fired and `cpu` claimed it: completed there FIRST (still
    /// enabled, so the controller takes the completion), then disabled — it cannot storm, and
    /// a later bind finds a line that can fire.
    #[must_use]
    pub fn quarantine(&mut self, cpu: Cpu) -> Steps {
        let mut steps = Steps::default();
        steps.push(Step::Complete(cpu));
        steps.push(Step::Disable(cpu));
        if let Some(other) = self.enabled_on.filter(|on| *on != cpu) {
            steps.push(Step::Disable(other));
        }
        *self = Self::UNBOUND;
        steps
    }

    /// The line's driver is gone (its endpoint died): a claim it can no longer complete is
    /// completed where it was made, and the line is disabled — the next driver that binds it
    /// finds a line that can fire.
    #[must_use]
    pub fn release(&mut self) -> Steps {
        let mut steps = Steps::default();
        if let Some(cpu) = self.claimed_on.take() {
            steps.push(Step::Complete(cpu));
        }
        self.wanted = None;
        self.settle(&mut steps);
        steps
    }

    /// Brings `enabled_on` to `wanted`: off in the old context, then on in the new — never
    /// two contexts at once (a line pending in between stays latched at the controller).
    fn settle(&mut self, steps: &mut Steps) {
        if self.enabled_on == self.wanted {
            return;
        }
        if let Some(old) = self.enabled_on {
            steps.push(Step::Disable(old));
        }
        if let Some(new) = self.wanted {
            steps.push(Step::Enable(new));
        }
        self.enabled_on = self.wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(s: Steps) -> ([Option<Step>; 3], usize) {
        let mut out = [None; 3];
        let mut n = 0;
        for step in s.iter() {
            out[n] = Some(step);
            n += 1;
        }
        (out, n)
    }

    fn list(s: Steps) -> std::vec::Vec<Step> {
        let (arr, n) = steps(s);
        arr[..n].iter().flatten().copied().collect()
    }

    /// A controller as the specification describes it: per-context enables, one gateway per
    /// line that forwards nothing between a claim and its completion, and a completion that
    /// is ignored unless the line is enabled for the context it is written to.
    #[derive(Default)]
    struct Controller {
        enabled: u64,
        gateway_busy: bool,
        ignored_completions: usize,
        both_enabled: bool,
    }

    impl Controller {
        fn apply(&mut self, s: Steps) {
            for step in s.iter() {
                match step {
                    Step::Enable(cpu) => self.enabled |= 1 << cpu,
                    Step::Disable(cpu) => self.enabled &= !(1 << cpu),
                    Step::Complete(cpu) => {
                        if self.enabled & (1 << cpu) != 0 {
                            self.gateway_busy = false;
                        } else {
                            self.ignored_completions += 1;
                        }
                    }
                }
                self.both_enabled |= self.enabled.count_ones() > 1;
            }
        }

        /// The CPU whose context takes the line if it fires now.
        fn fire(&mut self) -> Option<Cpu> {
            if self.gateway_busy || self.enabled == 0 {
                return None;
            }
            self.gateway_busy = true;
            Some(self.enabled.trailing_zeros() as Cpu)
        }
    }

    #[test]
    fn target_is_the_home_when_it_has_a_context() {
        assert_eq!(target(5, 0xff, |_| true), Some(5));
        assert_eq!(target(0, 0x1, |_| true), Some(0));
    }

    #[test]
    fn test_reject_target_without_a_context_or_offline() {
        // The home has no context (a hart the controller does not serve): the lowest that has.
        assert_eq!(target(3, 0xff, |cpu| cpu != 3), Some(0));
        // The boot hart has none either (a machine whose hart 0 is not ours to interrupt).
        assert_eq!(target(0, 0xff, |cpu| cpu >= 2), Some(2));
        // The home is offline: never chosen, whatever its context.
        assert_eq!(target(6, 0x0f, |_| true), Some(0));
        // A context on an offline CPU is no target.
        assert_eq!(target(1, 0b0101, |cpu| cpu == 1 || cpu == 2), Some(2));
        // No online CPU has a context: nowhere.
        assert_eq!(target(1, 0xff, |_| false), None);
        assert_eq!(target(1, 0, |_| true), None);
        // A CPU beyond the set is no home.
        assert_eq!(target(64, u64::MAX, |_| true), Some(0));
        assert_eq!(target(200, 0b10, |_| true), Some(1));
    }

    #[test]
    fn a_bind_enables_one_context_and_a_rebind_moves_it() {
        let mut route = Route::UNBOUND;
        assert_eq!(list(route.want(Some(2))), [Step::Enable(2)]);
        assert_eq!(route.enabled_on(), Some(2));
        // The same home again: nothing to do.
        assert_eq!(list(route.want(Some(2))), []);
        // The driver moved: off in the old context first, then on in the new.
        assert_eq!(list(route.want(Some(5))), [Step::Disable(2), Step::Enable(5)]);
        assert_eq!(route.enabled_on(), Some(5));
        // Nowhere to go: off.
        assert_eq!(list(route.want(None)), [Step::Disable(5)]);
        assert_eq!(route.enabled_on(), None);
    }

    #[test]
    fn a_claim_is_completed_where_it_was_made() {
        let mut route = Route::UNBOUND;
        let _ = route.want(Some(1));
        route.claimed(1);
        assert_eq!(route.claimed_on(), Some(1));
        assert_eq!(list(route.complete()), [Step::Complete(1)]);
        assert_eq!(route.claimed_on(), None);
        assert_eq!(route.enabled_on(), Some(1));
    }

    #[test]
    fn test_reject_move_under_an_outstanding_claim() {
        let mut route = Route::UNBOUND;
        let _ = route.want(Some(1));
        route.claimed(1);
        // The driver's home changes while CPU 1 holds the claim: no step — disabling CPU 1's
        // context now would make its completion a write the controller ignores.
        assert_eq!(list(route.want(Some(4))), []);
        assert_eq!(route.enabled_on(), Some(1));
        // A second change before the completion: still nothing, the last one counts.
        assert_eq!(list(route.want(Some(6))), []);
        // The completion goes to CPU 1, then the line moves to where it belongs now.
        assert_eq!(list(route.complete()), [Step::Complete(1), Step::Disable(1), Step::Enable(6)]);
        assert_eq!(route.enabled_on(), Some(6));
        assert_eq!(route.claimed_on(), None);
    }

    #[test]
    fn a_completion_without_a_claim_goes_to_the_enabled_context() {
        let mut route = Route::UNBOUND;
        assert_eq!(list(route.complete()), []);
        let _ = route.want(Some(3));
        assert_eq!(list(route.complete()), [Step::Complete(3)]);
    }

    #[test]
    fn an_unbound_line_is_completed_before_it_is_disabled() {
        // Never bound (the firmware left it enabled for CPU 0): CPU 0 claimed it.
        let mut route = Route::UNBOUND;
        route.claimed(0);
        assert_eq!(list(route.quarantine(0)), [Step::Complete(0), Step::Disable(0)]);
        assert_eq!(route, Route::UNBOUND);
        // A later bind finds a line whose gateway is free.
        let mut controller = Controller { enabled: 1, gateway_busy: true, ..Default::default() };
        let mut route = Route::UNBOUND;
        route.claimed(0);
        controller.apply(route.quarantine(0));
        assert_eq!(controller.ignored_completions, 0);
        controller.apply(route.want(Some(2)));
        assert_eq!(controller.fire(), Some(2));
    }

    #[test]
    fn a_quarantine_leaves_no_other_context_enabled() {
        let mut route = Route::UNBOUND;
        let _ = route.want(Some(4));
        // Its endpoint died and the claim came from a context the route did not expect.
        route.claimed(1);
        assert_eq!(
            list(route.quarantine(1)),
            [Step::Complete(1), Step::Disable(1), Step::Disable(4)]
        );
        assert_eq!(route, Route::UNBOUND);
    }

    #[test]
    fn a_dead_driver_leaves_a_line_the_next_one_can_use() {
        // The driver died between the delivery and its completion: CPU 2 holds the claim.
        let mut controller = Controller::default();
        let mut route = Route::UNBOUND;
        controller.apply(route.want(Some(2)));
        assert_eq!(controller.fire(), Some(2));
        route.claimed(2);
        let released = route.release();
        assert_eq!(list(released), [Step::Complete(2), Step::Disable(2)]);
        controller.apply(released);
        assert_eq!(route, Route::UNBOUND);
        assert_eq!(controller.ignored_completions, 0);
        // Nobody is bound: it does not fire.
        assert_eq!(controller.fire(), None);
        // The restarted driver binds from another CPU and is interrupted there.
        controller.apply(route.want(Some(5)));
        assert_eq!(controller.fire(), Some(5));
        // Without a claim the release only disables.
        let mut idle = Route::UNBOUND;
        let _ = idle.want(Some(1));
        assert_eq!(list(idle.release()), [Step::Disable(1)]);
        let mut unbound = Route::UNBOUND;
        assert_eq!(list(unbound.release()), []);
    }

    /// Ten thousand random events against the specified controller: the line is never enabled
    /// in two contexts, no completion is ignored, and a line that is wanted somewhere and not
    /// claimed can always fire — on the CPU it is wanted on.
    #[test]
    fn random_events_never_lose_or_double_a_line() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut route = Route::UNBOUND;
        let mut controller = Controller::default();
        let mut fired = 0usize;
        for _ in 0..10_000 {
            match next() % 4 {
                0 => {
                    let cpu = (next() % 9) as Cpu;
                    let wanted = if cpu == 8 { None } else { Some(cpu) };
                    controller.apply(route.want(wanted));
                }
                1 | 2 => {
                    if let Some(cpu) = controller.fire() {
                        assert_eq!(Some(cpu), route.enabled_on());
                        route.claimed(cpu);
                        fired += 1;
                    }
                }
                _ => {
                    if route.claimed_on().is_some() {
                        controller.apply(route.complete());
                    }
                }
            }
            assert!(!controller.both_enabled, "a line enabled in two contexts");
            assert_eq!(controller.ignored_completions, 0, "a completion the controller ignored");
            assert_eq!(controller.gateway_busy, route.claimed_on().is_some());
            if let Some(cpu) = route.enabled_on() {
                assert_eq!(controller.enabled, 1 << cpu);
            } else {
                assert_eq!(controller.enabled, 0);
            }
            if route.claimed_on().is_none() {
                assert_eq!(route.enabled_on(), route.wanted);
            }
        }
        assert!(fired > 1_000, "the model fired {fired} times");
    }
}
