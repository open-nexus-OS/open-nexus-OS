// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Theme qualifier for variant selection.
///
/// Resolution order (most specific first):
/// - `HighContrast` (accessibility override)
/// - `Dark` / `Light` (user preference)
/// - `Base` (always present, fallback for all tokens)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Qualifier {
    Base,
    Dark,
    Light,
    HighContrast,
}

impl Qualifier {
    /// Return the resolution chain for this qualifier.
    ///
    /// The chain is ordered from most specific to least specific.
    /// If a token is not found in the active qualifier, the runtime
    /// tries each subsequent entry until Base (which is always loaded).
    pub fn resolution_chain(&self) -> Vec<Qualifier> {
        match self {
            Qualifier::Base => vec![Qualifier::Base],
            Qualifier::Dark => vec![Qualifier::Dark, Qualifier::Base],
            Qualifier::Light => vec![Qualifier::Light, Qualifier::Base],
            Qualifier::HighContrast => {
                vec![Qualifier::HighContrast, Qualifier::Base]
            }
        }
    }
}
