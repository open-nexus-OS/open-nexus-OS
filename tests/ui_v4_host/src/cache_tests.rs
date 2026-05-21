// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6f host tests — specialized render caches (ShadowCache, TextCache, RenderCache).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 15 tests
//!
//! TEST SCOPE:
//!   - ShadowCache (insert/get, miss, update existing, LRU eviction, node invalidation, clear)
//!   - TextCache (insert/get, miss, LRU eviction, scale invalidation)
//!   - RenderCache (clear all sub-caches, dirty invalidation clears shadows only, scroll preserves,
//!     no-dirty no-op, begin_frame advances generation)
//!
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use nexus_effects::{RenderCache, ShadowArena, ShadowCache, TextCache};

    // ─── ShadowCache ───

    #[test]
    fn test_shadow_cache_insert_and_get() {
        let mut cache = ShadowCache::new();
        let data = vec![1u8, 2, 3, 4];
        cache.insert(0xABCD, data.clone(), 2, 2);
        assert_eq!(cache.len(), 1);
        let hit = cache.get(0xABCD);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_shadow_cache_miss() {
        let mut cache = ShadowCache::new();
        assert!(cache.get(0xDEAD).is_none());
    }

    #[test]
    fn test_shadow_cache_update_existing() {
        let mut cache = ShadowCache::new();
        cache.insert(1, vec![10], 1, 1);
        cache.insert(1, vec![20], 1, 1);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(1).unwrap(), &[20]);
    }

    #[test]
    fn test_shadow_cache_lru_eviction() {
        let mut cache = ShadowCache::with_capacity(2);
        cache.insert(1, vec![1], 1, 1);
        cache.insert(2, vec![2], 1, 1);
        // Access key 1 to make it recently used
        cache.get(1);
        // Insert 3 → should evict key 2 (least recently used)
        cache.insert(3, vec![3], 1, 1);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(1).is_some(), "key 1 should survive (recently accessed)");
        assert!(cache.get(3).is_some(), "key 3 should be present");
        assert!(cache.get(2).is_none(), "key 2 should be evicted (LRU)");
    }

    #[test]
    fn test_shadow_cache_invalidate_node() {
        let mut cache = ShadowCache::new();
        // Key format: high 32 bits = node_id_hash, low 32 = params
        cache.insert(0x0000_0001_0000_0001, vec![1], 1, 1);
        cache.insert(0x0000_0002_0000_0001, vec![2], 1, 1);
        cache.insert(0x0000_0002_0000_0002, vec![3], 1, 1);
        assert_eq!(cache.len(), 3);

        cache.invalidate_node(2); // invalidate all entries for node_id_hash=2
        assert_eq!(cache.len(), 1);
        assert!(cache.get(0x0000_0001_0000_0001).is_some());
    }

    #[test]
    fn test_shadow_cache_clear() {
        let mut cache = ShadowCache::new();
        cache.insert(1, vec![1], 1, 1);
        cache.insert(2, vec![2], 1, 1);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    // ─── TextCache ───

    #[test]
    fn test_text_cache_insert_and_get() {
        let mut cache = TextCache::new();
        let bitmap = vec![255u8; 16];
        cache.insert(0xAAAA, bitmap.clone(), 4, 4);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(0xAAAA).is_some());
    }

    #[test]
    fn test_text_cache_miss() {
        let mut cache = TextCache::new();
        assert!(cache.get(0xBBBB).is_none());
    }

    #[test]
    fn test_text_cache_lru_eviction() {
        let mut cache = TextCache::with_capacity(2);
        cache.insert(1, vec![1], 1, 1);
        cache.insert(2, vec![2], 1, 1);
        cache.insert(3, vec![3], 1, 1); // evicts key 1
        assert_eq!(cache.len(), 2);
        assert!(cache.get(1).is_none());
    }

    #[test]
    fn test_text_cache_invalidate_scale() {
        let mut cache = TextCache::new();
        // Key format: high 16 bits = scale_bucket
        cache.insert(0x0001_0000_0000_0001, vec![1], 1, 1);
        cache.insert(0x0002_0000_0000_0001, vec![2], 1, 1);
        cache.insert(0x0001_0000_0000_0002, vec![3], 1, 1);
        assert_eq!(cache.len(), 3);

        cache.invalidate_scale(1);
        assert_eq!(cache.len(), 1); // only scale_bucket=2 survives
    }

    // ─── RenderCache ───

    #[test]
    fn test_render_cache_clear() {
        let mut cache = RenderCache::new();
        cache.shadows.insert(1, vec![1], 1, 1);
        cache.text.insert(1, vec![2], 1, 1);
        cache.effects.insert(1, vec![3], 1, 1);
        cache.clear();
        assert!(cache.shadows.is_empty());
        assert!(cache.text.is_empty());
        assert!(cache.effects.is_empty());
    }

    #[test]
    fn test_render_cache_invalidate_dirty() {
        let mut cache = RenderCache::new();
        cache.shadows.insert(1, vec![1], 1, 1);
        cache.text.insert(1, vec![2], 1, 1);
        cache.invalidate_dirty(Some((0, 100)));
        assert!(cache.shadows.is_empty(), "shadows invalidated on dirty");
        assert!(!cache.text.is_empty(), "text survives dirty invalidation");
    }

    #[test]
    fn test_render_cache_scroll_preserves() {
        let mut cache = RenderCache::new();
        cache.shadows.insert(1, vec![1], 1, 1);
        cache.note_scroll();
        assert!(!cache.shadows.is_empty(), "scroll preserves shadow cache");
    }

    #[test]
    fn test_render_cache_no_dirty_no_invalidate() {
        let mut cache = RenderCache::new();
        cache.shadows.insert(1, vec![1], 1, 1);
        cache.invalidate_dirty(None);
        assert!(!cache.shadows.is_empty(), "no dirty → no invalidation");
    }

    #[test]
    fn test_render_cache_begin_frame() {
        let mut cache = RenderCache::new();
        let old_gen = cache.generation;
        cache.begin_frame();
        assert!(cache.generation > old_gen || cache.generation == old_gen.wrapping_add(1));
    }

    // ─── ShadowArena: Zero-Alloc Buffer Pool ───────────────────────────

    #[test]
    fn test_shadow_arena_alloc_and_reset() {
        let mut arena = ShadowArena::new();
        assert_eq!(arena.used_bytes(), 0);
        assert_eq!(arena.capacity(), 64 * 1024);

        let (off1, slice1) = arena.alloc(100).expect("first alloc");
        assert_eq!(off1, 0);
        assert_eq!(slice1.len(), 100);
        assert_eq!(arena.used_bytes(), 100);

        let (off2, slice2) = arena.alloc(200).expect("second alloc");
        assert_eq!(off2, 100);
        assert_eq!(slice2.len(), 200);
        assert_eq!(arena.used_bytes(), 300);

        // Reset: all allocations invalidated, used drops to 0
        arena.reset();
        assert_eq!(arena.used_bytes(), 0);

        // Can allocate again from start
        let (off3, _) = arena.alloc(50).expect("after reset");
        assert_eq!(off3, 0);
    }

    #[test]
    fn test_shadow_arena_overflow_returns_none() {
        let mut arena = ShadowArena::new();
        let cap = arena.capacity();

        // Fill the arena completely
        let (_, _) = arena.alloc(cap).expect("full capacity");

        // Next allocation must fail
        assert!(arena.alloc(1).is_none());
    }

    #[test]
    fn test_shadow_arena_get_retrieves_data() {
        let mut arena = ShadowArena::new();
        let (off, slice) = arena.alloc(8).expect("alloc 8 bytes");
        slice.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        let retrieved = arena.get(off, 8).expect("get within bounds");
        assert_eq!(retrieved, &[1, 2, 3, 4, 5, 6, 7, 8]);

        // Out of bounds
        assert!(arena.get(off, 9).is_none());
        assert!(arena.get(off + 1, 8).is_none());
    }

    #[test]
    fn test_shadow_arena_zero_len_alloc() {
        let mut arena = ShadowArena::new();
        let (off, slice) = arena.alloc(0).expect("zero-len alloc");
        assert_eq!(off, 0);
        assert_eq!(slice.len(), 0);
        assert_eq!(arena.used_bytes(), 0);
    }

    // ─── Alloc-Fail Prevention ─────────────────────────────────────────
    // These tests verify that the shadow rendering hot path produces
    // deterministic output without heap allocations.

    #[test]
    fn test_shadow_arena_no_heap_alloc_in_hot_path() {
        // ShadowArena::alloc() operates on a pre-allocated Vec<u8>.
        // The Vec is allocated once (at ShadowArena::new()).
        // Subsequent alloc() calls only bump the `used` pointer —
        // they do NOT touch the global allocator.
        //
        // This test verifies that repeated alloc/reset cycles
        // don't grow the underlying buffer or allocate.
        // We verify this by checking that capacity stays constant
        // and used_bytes returns to 0 after each cycle.

        let mut arena = ShadowArena::new();
        let cap = arena.capacity();

        // 100 alloc/reset cycles
        for i in 0..100 {
            let (off, _) = arena.alloc(1024).expect("alloc in cycle");
            assert!(off < cap, "offset must be within capacity (cycle {})", i);
            assert_eq!(arena.capacity(), cap, "capacity must not change");
            arena.reset();
            assert_eq!(arena.used_bytes(), 0, "used must be 0 after reset (cycle {})", i);
        }
    }

    #[test]
    fn test_alloc_fail_prevention_budget_check() {
        // Budget assertion: ShadowArena has a fixed capacity.
        // If an allocation would exceed capacity, it returns None
        // instead of panicking or hitting the global allocator.
        //
        // This is the OS-equivalent of preventing alloc-fail:
        // the caller must check the return value and degrade gracefully.

        let mut arena = ShadowArena::new();
        let cap = arena.capacity();

        // Fill to capacity - 1
        let _ = arena.alloc(cap - 1).expect("fill to cap-1");

        // Next allocation of 2 bytes must fail (only 1 byte free)
        assert!(arena.alloc(2).is_none());

        // Allocation of exactly remaining 1 byte must succeed
        let (off, _) = arena.alloc(1).expect("last byte");
        assert_eq!(off, cap - 1);

        // Now completely full
        assert!(arena.alloc(1).is_none());
    }

    #[test]
    fn test_alloc_fail_prevention_deterministic_reset() {
        // Determinism: after reset, the arena must produce
        // identical allocations as a fresh arena.
        let mut arena = ShadowArena::new();
        let (off1, _) = arena.alloc(100).expect("first");
        arena.reset();
        let (off2, _) = arena.alloc(100).expect("after reset");
        assert_eq!(off1, off2, "reset must produce same offset as fresh arena");

        // Multi-alloc determinism
        let mut a1 = ShadowArena::new();
        let _ = a1.alloc(10);
        let _ = a1.alloc(20);
        let _ = a1.alloc(30);
        a1.reset();
        let _ = a1.alloc(10);
        let _ = a1.alloc(20);
        let u1 = a1.used_bytes();

        let mut a2 = ShadowArena::new();
        let _ = a2.alloc(10);
        let _ = a2.alloc(20);
        let _ = a2.alloc(30);
        a2.reset();
        let _ = a2.alloc(10);
        let _ = a2.alloc(20);
        let u2 = a2.used_bytes();

        assert_eq!(u1, u2, "identical usage patterns must produce identical used_bytes");
    }
}
