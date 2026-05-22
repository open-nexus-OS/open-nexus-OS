// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Animation property types — which layer properties can be animated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimProp {
    Opacity,
    TranslateX,
    TranslateY,
    ScaleX,
    ScaleY,
    ShadowRadius,
    BlurRadius,
}

/// Identifies a compositor layer for animation targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

/// Output of one animation tick: a changed property value for a specific layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneUpdate {
    pub layer_id: LayerId,
    pub property: AnimProp,
    /// Interpolated value at current time (0.0..1.0 for opacity/scale, px for translate/radius)
    pub value: f32,
    /// Progress through the animation (0.0 = start, 1.0 = complete)
    pub progress: f32,
}
