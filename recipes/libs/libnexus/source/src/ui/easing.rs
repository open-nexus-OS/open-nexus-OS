/// Easing functions for smooth animations
/// Based on CSS easing functions and common animation curves

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Linear interpolation (no easing)
    Linear,
    /// Ease in (slow start)
    EaseIn,
    /// Ease out (slow end)
    EaseOut,
    /// Ease in-out (slow start and end)
    EaseInOut,
    /// Cubic ease in
    CubicIn,
    /// Cubic ease out
    CubicOut,
    /// Cubic ease in-out
    CubicInOut,
    /// Bounce effect
    Bounce,
    /// Elastic effect
    Elastic,
}

impl Default for Easing {
    fn default() -> Self {
        Easing::CubicOut
    }
}

impl Easing {
    /// Apply easing function to a value between 0.0 and 1.0
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - 2.0 * (1.0 - t) * (1.0 - t)
                }
            }
            Easing::CubicIn => t * t * t,
            Easing::CubicOut => 1.0 - (1.0 - t).powi(3),
            Easing::CubicInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - 4.0 * (1.0 - t).powi(3)
                }
            }
            Easing::Bounce => {
                if t < 1.0 / 2.75 {
                    7.5625 * t * t
                } else if t < 2.0 / 2.75 {
                    let t = t - 1.5 / 2.75;
                    7.5625 * t * t + 0.75
                } else if t < 2.5 / 2.75 {
                    let t = t - 2.25 / 2.75;
                    7.5625 * t * t + 0.9375
                } else {
                    let t = t - 2.625 / 2.75;
                    7.5625 * t * t + 0.984375
                }
            }
            Easing::Elastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let c4 = (2.0 * std::f32::consts::PI) / 3.0;
                    -(2.0_f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * c4).sin()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_easing_bounds() {
        let easing = Easing::CubicOut;
        assert_eq!(easing.apply(0.0), 0.0);
        assert_eq!(easing.apply(1.0), 1.0);
    }

    #[test]
    fn test_easing_clamping() {
        let easing = Easing::Linear;
        assert_eq!(easing.apply(-0.5), 0.0);
        assert_eq!(easing.apply(1.5), 1.0);
    }
}
