//! Capability system abstractions.

/// Capability identifier placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability(pub u64);

/// Capability table entry linking typed handles.
pub struct Entry<T> {
    pub cap: Capability,
    pub object: T,
}

impl<T> Entry<T> {
    pub fn new(cap: Capability, object: T) -> Self {
        Self { cap, object }
    }
}
