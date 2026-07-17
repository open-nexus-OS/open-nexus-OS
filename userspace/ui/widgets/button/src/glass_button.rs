// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GlassButton` — the design-system button (handoff `GlassButton`): 6 variants
//! × 4 sizes × interaction state, resolved from theme tokens. A pure builder
//! that produces the styled container `LayoutNode` (delegating the box to the
//! low-level [`Button`] primitive) and exposes the resolved foreground so the
//! caller colors the label/icon — widgets don't own text (see [`Button`]).
//! DSL-emittable: `GlassButton{variant:.glass,size:.md}` maps 1:1 to this builder.

use nexus_layout_types::{FxPx, LayoutNode, Rgba8};
use nexus_style::{blend, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

use crate::Button;

/// Translucency of the glass variant's fill (0..255).
const GLASS_FILL_ALPHA: u8 = 170;
/// Backdrop blur + saturation for the glass variant.
const GLASS_BLUR_RADIUS: u32 = 20;
const GLASS_SATURATION: u32 = 140;

/// Visual variant (handoff `GlassButtonProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Filled primary action.
    #[default]
    Default,
    /// Frosted translucent glass.
    Glass,
    /// Transparent; fills only on hover/press.
    Ghost,
    /// Filled danger action.
    Destructive,
    /// Filled accent with a persistent "on" emphasis border.
    Active,
    /// Neutral filled.
    Secondary,
}

/// Size preset (handoff `GlassButtonProps.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Sm,
    #[default]
    Md,
    Lg,
    /// Square icon button.
    Icon,
}

impl ButtonSize {
    /// Uniform inner padding.
    pub fn padding(self) -> FxPx {
        FxPx::new(match self {
            ButtonSize::Sm => 6,
            ButtonSize::Md => 10,
            ButtonSize::Lg => 14,
            ButtonSize::Icon => 8,
        })
    }

    /// Corner radius token.
    pub fn radius(self) -> LengthToken {
        match self {
            ButtonSize::Lg => LengthToken::RadiusMedium,
            _ => LengthToken::RadiusSmall,
        }
    }
}

/// The design-system button. Build with [`GlassButton::build`] (needs the theme).
#[derive(Debug, Clone, Default)]
pub struct GlassButton {
    variant: ButtonVariant,
    size: ButtonSize,
    state: InteractionState,
    id: Option<&'static str>,
    content: Option<LayoutNode>,
}

impl GlassButton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    /// Interaction id (the compositor hit-tests the rendered rect by this id).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The label/icon node (caller-provided, colored with [`Self::foreground`]).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// The resolved foreground (label/icon) color for this variant.
    pub fn foreground(&self, tokens: &dyn Tokens) -> Rgba8 {
        let fg = match self.variant {
            ButtonVariant::Default | ButtonVariant::Active => ColorToken::OnAccent,
            ButtonVariant::Destructive => ColorToken::OnDanger,
            ButtonVariant::Glass | ButtonVariant::Ghost | ButtonVariant::Secondary => {
                ColorToken::OnSurface
            }
        };
        tokens.color(fg)
    }

    /// The container background for the current variant + state (None = transparent).
    fn background(&self, tokens: &dyn Tokens) -> Option<Rgba8> {
        let fg = self.foreground(tokens);
        let wash = self.state.wash_alpha();
        match self.variant {
            ButtonVariant::Ghost => {
                // Transparent at rest; a fg-tinted wash on hover/press.
                (wash > 0).then(|| Rgba8::new(fg.r, fg.g, fg.b, wash))
            }
            variant => {
                let base = match variant {
                    ButtonVariant::Default | ButtonVariant::Active => {
                        tokens.color(ColorToken::Accent)
                    }
                    ButtonVariant::Destructive => tokens.color(ColorToken::Danger),
                    ButtonVariant::Secondary => tokens.color(ColorToken::SurfaceVariant),
                    ButtonVariant::Glass => {
                        let mut c = tokens.color(ColorToken::Surface);
                        c.a = GLASS_FILL_ALPHA;
                        c
                    }
                    ButtonVariant::Ghost => unreachable!(),
                };
                Some(if wash > 0 { blend(base, fg, wash) } else { base })
            }
        }
    }

    /// The resolved container [`Style`] (background/border/rounded/blur/opacity)
    /// for the current variant + size + state.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let mut s = Style::new();
        if let Some(bg) = self.background(tokens) {
            s = s.background(bg);
        }
        s = s.rounded(tokens.length(self.size.radius()));

        // Borders: glass + active carry a hairline; focus overrides with the ring.
        if self.state.shows_focus_ring() {
            s = s.border_token(tokens, LengthToken::BorderThin, ColorToken::FocusRing);
        } else if matches!(self.variant, ButtonVariant::Glass) {
            s = s.border_token(tokens, LengthToken::BorderThin, ColorToken::Border);
        } else if matches!(self.variant, ButtonVariant::Active) {
            s = s.border_token(tokens, LengthToken::BorderThin, ColorToken::Accent);
        }

        if matches!(self.variant, ButtonVariant::Glass) {
            s = s.blur(GLASS_BLUR_RADIUS, GLASS_SATURATION);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity());
        }
        s
    }

    /// Build the styled container node (centers the content).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.style(tokens);
        let mut button = Button::new().style(style).padding(self.size.padding());
        if let Some(id) = self.id {
            button = button.id(id);
        }
        if let Some(content) = self.content {
            button = button.content(content);
        }
        button.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    #[test]
    fn variant_backgrounds_come_from_tokens() {
        let t = BaseTokens;
        let default_bg = GlassButton::new().variant(ButtonVariant::Default).background(&t);
        assert_eq!(default_bg, Some(t.color(ColorToken::Accent)));
        let destructive_bg = GlassButton::new().variant(ButtonVariant::Destructive).background(&t);
        assert_eq!(destructive_bg, Some(t.color(ColorToken::Danger)));
        // Ghost is transparent at rest.
        assert_eq!(GlassButton::new().variant(ButtonVariant::Ghost).background(&t), None);
    }

    #[test]
    fn glass_variant_is_translucent_and_blurred() {
        let t = DarkTokens;
        let b = GlassButton::new().variant(ButtonVariant::Glass);
        assert_eq!(b.background(&t).map(|c| c.a), Some(GLASS_FILL_ALPHA));
        assert!(b.style(&t).backdrop_blur().is_some());
    }

    #[test]
    fn hover_washes_toward_foreground() {
        let t = BaseTokens;
        let rest = GlassButton::new().variant(ButtonVariant::Default).background(&t).unwrap();
        let hover = GlassButton::new()
            .variant(ButtonVariant::Default)
            .state(InteractionState::Hover)
            .background(&t)
            .unwrap();
        assert_ne!(rest, hover, "hover should shift the fill");
    }

    #[test]
    fn disabled_dims_and_focus_adds_ring() {
        let t = BaseTokens;
        let disabled = GlassButton::new().state(InteractionState::Disabled).style(&t);
        assert_eq!(disabled.visual().opacity, Some(InteractionState::Disabled.opacity()));
        let focused = GlassButton::new().state(InteractionState::Focused).style(&t);
        assert!(focused.visual().border.top.is_some(), "focus ring border");
    }

    #[test]
    fn build_delegates_to_button_with_id() {
        let t = BaseTokens;
        let node = GlassButton::new().id("open").variant(ButtonVariant::Glass).build(&t);
        match node {
            LayoutNode::Stack(stack, visual, _) => {
                assert_eq!(stack.id, Some("open"));
                assert!(visual.background.is_some());
            }
            _ => panic!("GlassButton must build a Stack"),
        }
    }
}
