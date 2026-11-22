// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The glass writes of a frame counted by what reached the glass (TASK-0351 P6, `gpud:
//! glass writes by`) — beside `Writes`, which counts them by cause — and the key misses at rest by
//! what differs (P19 W5) and, written whole, by the first condition that kept them from a band
//! (package B 2b), the owe split by its source (P19-e: `gpud: glass rest misses (… owe=…)`).
//! Moved out of `reach` (its rules stay there; a child module reads `Touch`'s fields).
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; `tests/frame_rest_misses.rs` (the counters through a plan)
//! INVARIANTS: counters only — never an input to a rule; every count saturates.

use super::super::damage::Area;
use super::super::glass::{FullBy, Miss, WriteCause};
use super::Touch;

/// The glass writes of a frame by what reached the glass (TASK-0351 P6: `gpud: glass writes
/// by`) — beside [`crate::frame::Writes`], which counts them by cause. A write has one class or
/// none: a full present, new rows and a put-off write paid later carry none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WritesBy {
    /// A band: a named change of a layer below the glass.
    pub below: u16,
    /// A band: a glass below it was blurred again (R3), and nothing named reached it.
    pub glass: u16,
    /// A band: a change of origin 0 or of a key the set does not hold reached it — a call site
    /// that queues damage without its layer. The sum of `zero` and `stray`.
    pub unknown: u16,
    /// Of `unknown`: a change of origin 0 reached it (TASK-0351 P19, W4: windowd queued it
    /// without its layer — `windowd: damage zero (…)` names where).
    pub zero: u16,
    /// Of `unknown`: only a change of a key no layer of the set carries reached it (a culled or
    /// gone layer's key).
    pub stray: u16,
    /// The backdrop: a layer below the glass came, went or moved.
    pub moved: u16,
    /// Layers below the glass swapped their order (R4).
    pub order: u16,
    /// The one write after the glass kept its rows through a motion.
    pub rest: u16,
    /// Of the `rest` writes, those of a key miss, by what differs (TASK-0351 P19, W5).
    pub misses: RestMisses,
    /// Of those, the ones written as the whole glass, by the first condition that kept them
    /// from a band (TASK-0351 P19, package B 2b: the rule that cuts them is chosen by this).
    pub whole: RestWhole,
}

/// Why a key miss at rest was written as the whole glass, the first condition that fired
/// (`gpud: glass rest misses (… whole=base/owe/new/shape)`) — a counter, no rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestWhole {
    /// The base the rows were blurred from is not this one (the seed, the glass's own
    /// rectangle or radii).
    pub base: u16,
    /// The owe, or what the hold collected, is the whole glass — the sum of `owe_by`.
    pub owe: u16,
    /// The glass is new to this plan (no change set places it).
    pub new: u16,
    /// The band the changes explain is not one rectangle.
    pub shape: u16,
    /// The `owe` writes by the source that was the whole glass (P19-e).
    pub owe_by: OweWhole,
}

/// The whole rest writes of a whole owe by its first source that is the whole glass (TASK-0351
/// P19-e, `owe=full/lost/owed/displaced owed_by=put_off/lost/placed/displaced`) — a counter, no
/// rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OweWhole {
    /// A full present at the rest plan.
    pub full: u16,
    /// The rest plan's change list found no room.
    pub lost: u16,
    /// What the rows owed was the whole glass.
    pub owed: u16,
    /// What the held plans displaced was (a plan that did not compute the glass).
    pub displaced: u16,
    /// Of `owed`: made whole by a write put off on a key miss nothing explained.
    pub owed_put_off: u16,
    /// Of `owed`: made whole by a hold on a key miss whose change list was lost.
    pub owed_lost: u16,
    /// Of `owed`: made whole by a hold on a key hit whose change list was lost.
    pub owed_placed: u16,
    /// Of `owed`: made whole by a hold owing a change while what moved below was not followed.
    pub owed_displaced: u16,
}

/// The first source of a whole owe at a rest write, in `OweWhole`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::frame) enum OweFull {
    Full,
    Lost,
    Owed(Option<FullBy>),
    Displaced,
}

impl OweFull {
    /// The first source that is the whole glass — `full`: a full present; `touch`: the changes
    /// that reach the glass; `owed`: what its rows owe, made whole by `by`; `displaced`: what the
    /// held plans displaced — or `None` when the owe is bounded.
    pub(in crate::frame) fn of(
        full: bool,
        touch: Area,
        owed: Area,
        displaced: Area,
        by: Option<FullBy>,
    ) -> Option<Self> {
        if full {
            Some(Self::Full)
        } else if touch == Area::Full {
            Some(Self::Lost)
        } else if owed == Area::Full {
            Some(Self::Owed(by))
        } else {
            (displaced == Area::Full).then_some(Self::Displaced)
        }
    }
}

impl RestWhole {
    /// Counts the first condition that fired, in the order above; `owe`: the whole owe's
    /// source (`OweFull::of`), `None` for a bounded owe.
    pub(in crate::frame) fn note(&mut self, same_base: bool, owe: Option<OweFull>, placed: bool) {
        let o = &mut self.owe_by;
        let n = match owe {
            _ if !same_base => &mut self.base,
            Some(src) => {
                let (by, sub) = match src {
                    OweFull::Full => (&mut o.full, None),
                    OweFull::Lost => (&mut o.lost, None),
                    OweFull::Displaced => (&mut o.displaced, None),
                    OweFull::Owed(by) => (
                        &mut o.owed,
                        by.map(|b| match b {
                            FullBy::PutOff => &mut o.owed_put_off,
                            FullBy::Lost => &mut o.owed_lost,
                            FullBy::Placed => &mut o.owed_placed,
                            FullBy::Displaced => &mut o.owed_displaced,
                        }),
                    ),
                };
                *by = by.saturating_add(1);
                if let Some(sub) = sub {
                    *sub = sub.saturating_add(1);
                }
                &mut self.owe
            }
            None if !placed => &mut self.new,
            None => &mut self.shape,
        };
        *n = n.saturating_add(1);
    }
}

/// The writes that end a hold with a key miss, by the first of the glass's prints that differs
/// from the ones its rows were written with (TASK-0351 P19, W5: `gpud: glass rest misses`) —
/// counters, never an input to a rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestMisses {
    /// Layers below within its reach came, went or changed their order (or the rows carry no
    /// prints).
    pub set: u16,
    /// Only layers of the glass's own window moved or resized.
    pub own: u16,
    /// Layers of other windows below it moved or resized.
    pub moved: u16,
    /// The same layers at the same places: something else the key folds changed (an opacity, a
    /// source, a corner radius, the glass's own rectangle or radii).
    pub drew: u16,
    /// The wallpaper the frames start from was uploaded again.
    pub epoch: u16,
    /// Of all of these, the ones written as a band (C-r: what changed below while the rows
    /// were kept, `Frame::plan`) that blurs less than the whole glass — a band that reaches all
    /// of it costs the whole glass and is not counted. Never more than their sum.
    pub band: u16,
}

impl RestMisses {
    fn note(&mut self, m: Miss) {
        let n = match m {
            Miss::Set => &mut self.set,
            Miss::Own => &mut self.own,
            Miss::Moved => &mut self.moved,
            Miss::Drew => &mut self.drew,
            Miss::Epoch => &mut self.epoch,
        };
        *n = n.saturating_add(1);
    }
}

impl WritesBy {
    /// Counts one write of `cause` for what `touch` says reached the glass; `rested`: the last
    /// plan kept the glass's rows through a motion; `differs`: what differs from the prints the
    /// rows were written with (`GlassCache::printed`), counted for a key miss at rest.
    pub(in crate::frame) fn note(
        &mut self,
        touch: &Touch,
        cause: WriteCause,
        rested: bool,
        differs: Option<Miss>,
    ) {
        if let (true, WriteCause::Backdrop, Some(m)) = (rested, cause, differs) {
            self.misses.note(m);
        }
        let n = match cause {
            WriteCause::Full => return,
            _ if rested => &mut self.rest,
            WriteCause::Backdrop if touch.order => &mut self.order,
            WriteCause::Backdrop => &mut self.moved,
            WriteCause::Band if touch.base || touch.stray => {
                let split = if touch.base { &mut self.zero } else { &mut self.stray };
                *split = split.saturating_add(1);
                &mut self.unknown
            }
            WriteCause::Band if touch.below => &mut self.below,
            WriteCause::Band if touch.glass => &mut self.glass,
            WriteCause::Band if touch.order => &mut self.order,
            _ => return,
        };
        *n = n.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_is_counted_once_by_what_reached_it() {
        let mut by = WritesBy::default();
        let t = |f: fn(&mut Touch)| {
            let mut t = Touch::NONE;
            f(&mut t);
            t
        };
        by.note(&t(|t| (t.base, t.below) = (true, true)), WriteCause::Band, false, None);
        by.note(&t(|t| (t.below, t.glass) = (true, true)), WriteCause::Band, false, None);
        by.note(&t(|t| t.glass = true), WriteCause::Band, false, None);
        by.note(&t(|t| t.order = true), WriteCause::Backdrop, false, None);
        by.note(&t(|_| {}), WriteCause::Backdrop, false, None);
        by.note(&t(|t| t.below = true), WriteCause::Backdrop, true, None);
        by.note(&Touch::FULL, WriteCause::Full, true, None);
        by.note(&t(|_| {}), WriteCause::New, false, None);
        let all = WritesBy {
            below: 1,
            glass: 1,
            unknown: 1,
            zero: 1,
            moved: 1,
            order: 1,
            rest: 1,
            ..Default::default()
        };
        assert_eq!(by, all);
    }

    #[test]
    fn an_unknown_origin_is_counted_as_zero_or_stray_and_their_sum_is_unknown() {
        let mut by = WritesBy::default();
        let (zero, stray, both) = (
            Touch { base: true, ..Touch::NONE },
            Touch { stray: true, ..Touch::NONE },
            Touch { base: true, stray: true, below: true, ..Touch::NONE },
        );
        for t in [zero, stray, both, stray] {
            by.note(&t, WriteCause::Band, false, None);
        }
        assert_eq!((by.zero, by.stray, by.unknown, by.below), (2, 2, 4, 0));
        // A write at rest is `rest` whatever reached it: neither zero nor stray.
        by.note(&zero, WriteCause::Band, true, None);
        assert_eq!((by.zero + by.stray, by.unknown, by.rest), (4, 4, 1));
    }

    #[test]
    fn a_key_miss_at_rest_is_counted_by_what_differs_and_no_other_write_is() {
        let mut by = WritesBy::default();
        let none = Touch::NONE;
        for m in [Miss::Set, Miss::Own, Miss::Moved, Miss::Drew, Miss::Epoch, Miss::Set] {
            by.note(&none, WriteCause::Backdrop, true, Some(m));
        }
        // Not at rest, not a key miss, or nothing to compare: not a rest miss.
        by.note(&none, WriteCause::Backdrop, false, Some(Miss::Own));
        by.note(&none, WriteCause::Band, true, Some(Miss::Own));
        by.note(&none, WriteCause::New, true, Some(Miss::Own));
        by.note(&none, WriteCause::Full, true, Some(Miss::Own));
        by.note(&none, WriteCause::Backdrop, true, None);
        let misses = RestMisses { set: 2, own: 1, moved: 1, drew: 1, epoch: 1, band: 0 };
        assert_eq!((by.misses, by.rest), (misses, 9));
    }

    #[test]
    fn a_whole_rest_write_counts_its_first_condition() {
        let mut w = RestWhole::default();
        let owed = Some(OweFull::Owed(Some(FullBy::PutOff)));
        w.note(false, owed, false);
        w.note(true, owed, false);
        w.note(true, None, false);
        w.note(true, None, true);
        let owe_by = OweWhole { owed: 1, owed_put_off: 1, ..OweWhole::default() };
        assert_eq!(w, RestWhole { base: 1, owe: 1, new: 1, shape: 1, owe_by });
    }

    #[test]
    fn a_whole_owe_is_counted_by_its_first_whole_source() {
        let (f, n, r) = (Area::Full, Area::None, Area::rect(0, 0, 4, 4));
        assert_eq!(OweFull::of(true, f, f, f, None), Some(OweFull::Full));
        assert_eq!(OweFull::of(false, f, f, f, None), Some(OweFull::Lost));
        let lost = Some(FullBy::Lost);
        assert_eq!(OweFull::of(false, r, f, f, lost), Some(OweFull::Owed(lost)));
        assert_eq!(OweFull::of(false, n, r, f, lost), Some(OweFull::Displaced));
        assert_eq!(OweFull::of(false, r, r, r, lost), None, "a bounded owe");
        let mut w = RestWhole::default();
        let rules = [FullBy::PutOff, FullBy::Lost, FullBy::Placed, FullBy::Displaced];
        for by in core::iter::once(None).chain(rules.map(Some)) {
            w.note(true, Some(OweFull::Owed(by)), true);
        }
        for src in [OweFull::Full, OweFull::Lost, OweFull::Displaced] {
            w.note(true, Some(src), true);
        }
        let owe_by = OweWhole {
            full: 1,
            lost: 1,
            owed: 5,
            displaced: 1,
            owed_put_off: 1,
            owed_lost: 1,
            owed_placed: 1,
            owed_displaced: 1,
        };
        assert_eq!(w, RestWhole { owe: 8, owe_by, ..RestWhole::default() });
    }
}
