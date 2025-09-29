pub mod backgrounds;
pub mod themes;
pub mod ui;

// This lets the rest of your code keep using `use libnexus::{THEME, IconVariant};`
pub use themes::{
    THEME,
    ThemeManager,
    ThemeId,
    IconVariant,
    Paint,
    Acrylic,
};

// UI system exports
pub use ui::{
    // Animation system
    Timeline,
    Direction,
    Easing,
    AnimationManager,
    AnimationDuration,
    AnimationProgress,
    AnimationState,
    AnimationConfig,
    AnimationMetadata,
    AnimationResult,
    // Redox Animation Timer
    animation::redox_timer::RedoxAnimationTimer,
    // Layout system
    Insets,
    ScreenDimensions,
    Rect,
    HorizontalAlignment,
    VerticalAlignment,
};
