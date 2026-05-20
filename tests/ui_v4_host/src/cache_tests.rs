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
    use nexus_effects::{RenderCache, ShadowCache, TextCache};

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
}
