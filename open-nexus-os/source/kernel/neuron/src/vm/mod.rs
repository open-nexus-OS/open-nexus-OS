//! Virtual memory management hooks.

pub struct Region {
    pub base: usize,
    pub size: usize,
}

impl Region {
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }
}

pub fn map(_region: &Region) {
    // Pager integration will install stage-two mappings here.
}
