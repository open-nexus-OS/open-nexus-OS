//! Scheduler frameworks for NEURON.

/// Scheduling class enumeration covering CFS-like, EDF, and fixed priority domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    CfsLike,
    EarliestDeadlineFirst,
    FixedPriority,
}

/// Scheduling entity placeholder.
pub struct Entity {
    pub class: Class,
}

impl Entity {
    pub const fn new(class: Class) -> Self {
        Self { class }
    }
}

pub fn enqueue(_entity: &Entity) {
    // Queue entities into the appropriate run-queue per scheduling class.
}
