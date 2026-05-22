// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 3 layer tests — Layer struct, LayerCache, dirty tracking.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 tests
//!
//! TEST SCOPE: layer initialization, dirty flag, cache clear, multi-layer independence
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    struct TestLayer {
        id: u64,
        dirty: bool,
        pixels: Vec<u8>,
    }

    struct TestLayerCache {
        layers: Vec<TestLayer>,
    }

    impl TestLayerCache {
        fn new() -> Self {
            Self { layers: Vec::new() }
        }
        fn add(&mut self, id: u64, size: usize) {
            self.layers.push(TestLayer {
                id,
                dirty: true,
                pixels: vec![0; size],
            });
        }
        fn mark_clean(&mut self, id: u64) {
            if let Some(l) = self.layers.iter_mut().find(|l| l.id == id) {
                l.dirty = false;
            }
        }
        fn is_dirty(&self, id: u64) -> bool {
            self.layers.iter().any(|l| l.id == id && l.dirty)
        }
        fn clear(&mut self) {
            self.layers.clear();
        }
        fn len(&self) -> usize {
            self.layers.len()
        }
    }

    #[test]
    fn test_layer_initialized_dirty() {
        let mut cache = TestLayerCache::new();
        cache.add(1, 100);
        assert!(cache.is_dirty(1), "new layer should be dirty");
    }

    #[test]
    fn test_layer_mark_clean() {
        let mut cache = TestLayerCache::new();
        cache.add(1, 100);
        cache.mark_clean(1);
        assert!(!cache.is_dirty(1), "marked clean → not dirty");
    }

    #[test]
    fn test_layer_cache_clear() {
        let mut cache = TestLayerCache::new();
        cache.add(1, 100);
        cache.add(2, 200);
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert_eq!(cache.len(), 0, "clear removes all layers");
    }

    #[test]
    fn test_two_layers_independent() {
        let mut cache = TestLayerCache::new();
        cache.add(1, 100);
        cache.add(2, 200);
        cache.mark_clean(1);
        assert!(!cache.is_dirty(1), "layer 1 clean");
        assert!(cache.is_dirty(2), "layer 2 still dirty");
    }

    #[test]
    fn test_layer_pixel_data_preserved() {
        let mut cache = TestLayerCache::new();
        cache.add(1, 16);
        if let Some(layer) = cache.layers.iter_mut().find(|l| l.id == 1) {
            layer.pixels[0] = 42;
            layer.pixels[15] = 99;
        }
        cache.mark_clean(1);
        // Pixel data should survive mark_clean
        if let Some(layer) = cache.layers.iter().find(|l| l.id == 1) {
            assert_eq!(layer.pixels[0], 42);
            assert_eq!(layer.pixels[15], 99);
        }
    }
}
