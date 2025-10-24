#![cfg_attr(not(test), no_std)]

//! CONTEXT: Minimal bump allocator for deterministic testing
//! OWNERS: @runtime
//! PUBLIC API: BumpAllocator
//! DEPENDS_ON: core
//! INVARIANTS: No heap growth; alignment respected; test-only in most crates
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

/// Very small bump allocator stub for deterministic testing.
pub struct BumpAllocator {
    start: usize,
    end: usize,
    cursor: usize,
}

impl BumpAllocator {
    pub const fn new(start: usize, size: usize) -> Self {
        Self { start, end: start + size, cursor: start }
    }

    pub fn alloc(&mut self, len: usize, align: usize) -> Option<usize> {
        let align_mask = align - 1;
        let aligned = (self.cursor + align_mask) & !align_mask;
        let next = aligned.checked_add(len)?;
        if next > self.end {
            return None;
        }
        self.cursor = next;
        Some(aligned)
    }

    pub fn reset(&mut self) {
        self.cursor = self.start;
    }
}

#[cfg(test)]
mod tests {
    use super::BumpAllocator;

    #[test]
    fn allocates_aligned_chunks() {
        let mut bump = BumpAllocator::new(0, 64);
        let first = bump.alloc(8, 8).expect("fits");
        assert_eq!(first % 8, 0);
        let second = bump.alloc(16, 16).expect("fits");
        assert_eq!(second % 16, 0);
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut bump = BumpAllocator::new(0, 8);
        assert!(bump.alloc(16, 8).is_none());
    }
}
