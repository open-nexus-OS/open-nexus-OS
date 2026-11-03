// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The registry's vocabulary (RFC-0113 §7) — a chord is an EXACT modifier set and one
//! key; an entry names what acts on it. Split from `lib.rs` so the table reads as data.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/registry.rs`

use hid::KeyboardUsage;

/// The modifiers of a chord — the bit layout of the surface wire's modifier byte
/// (`nexus_display_proto::surface_input::MOD_*`, pinned by inputd's tests). A chord matches only
/// the very set it names: Super+Ctrl+← is not Super+Ctrl+Shift+←.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods(u8);

impl Mods {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CTRL: Self = Self(2);
    /// Left Alt.
    pub const ALT: Self = Self(4);
    /// Right Alt as AltGr (the third level of a layout).
    pub const ALTGR: Self = Self(8);
    pub const SUPER: Self = Self(16);
    const ALL: u8 = 0x1f;

    /// From the wire byte; bits no modifier names are dropped.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & Self::ALL)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Both sets held at once.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The key of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A key by its place (HID usage) — the arrows, Home, Print: the same key in every layout.
    Usage(KeyboardUsage),
    /// A key by the letter the ACTIVE layout types on it (lowercase ASCII): Ctrl+Z is the key
    /// labelled Z, wherever the layout puts it (the German layout swaps Z and Y). A layout that
    /// types no Latin letter there falls back to the key's US letter, so every chord works in
    /// every layout (`keymaps::Keymap::letter`).
    Letter(char),
}

/// An exact modifier set and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

impl Chord {
    /// Whether a press of `usage` — on which the active layout types `letter` — with exactly
    /// `mods` held is this chord.
    #[must_use]
    pub fn matches(&self, mods: Mods, usage: KeyboardUsage, letter: Option<char>) -> bool {
        self.mods == mods
            && match self.key {
                Key::Usage(key) => key == usage,
                Key::Letter(key) => letter == Some(key),
            }
    }
}

/// The settings page's categories, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Windows,
    Screenshots,
    Text,
    Input,
}

impl Category {
    pub const ALL: [Self; 4] = [Self::Windows, Self::Screenshots, Self::Text, Self::Input];

    /// The id the page passes (`svc.shortcuts.list(category, …)`).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Screenshots => "screenshots",
            Self::Text => "text",
            Self::Input => "input",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// The editing commands a keyboard sends a focused text field (TASK-0067B, TASK-0346).
/// Composition never consumes one — imed passes it to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Left,
    Right,
    Home,
    End,
    SelectLeft,
    SelectRight,
    SelectHome,
    SelectEnd,
    Delete,
    SelectAll,
    Copy,
    Cut,
    Paste,
    /// By words; the `Select…` forms extend the selection.
    WordLeft,
    WordRight,
    SelectWordLeft,
    SelectWordRight,
    /// A word at a time.
    DeleteWordBack,
    DeleteWordForward,
    Undo,
    Redo,
}

/// What a match does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A window command: windowd's tiling code (`zones::CODE_*`, pinned by windowd's tests),
    /// carried as a key-channel event (`KEY_INPUT_CHORD`).
    Window(u8),
    /// A screen capture, a key-channel event (`KEY_INPUT_CAPTURE`) — 1 the screenshot UI, 2 the
    /// screen, 3 the focused window.
    Capture(u8),
    /// A command for the focused text field.
    Field(EditKey),
    /// The focus moves to the next field (the keymap's `Tab`).
    FocusNext,
    /// The focus moves to the field before (the keymap's `BackTab`).
    FocusPrevious,
    /// The next input source (keyboard layout or input method).
    NextInputSource,
}

/// The component that acts on a match (RFC-0113 §7's `owner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The compositor: tiling, minimize, close.
    Windowd,
    /// The desktop shell with screencapd: the capture UI and the saved shots.
    Shell,
    /// The focused text field (the app's editing engine).
    Field,
    /// The input service: the input sources.
    Inputd,
}

impl Action {
    #[must_use]
    pub const fn owner(self) -> Owner {
        match self {
            Self::Window(_) => Owner::Windowd,
            Self::Capture(_) => Owner::Shell,
            Self::Field(_) | Self::FocusNext | Self::FocusPrevious => Owner::Field,
            Self::NextInputSource => Owner::Inputd,
        }
    }
}

/// One registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// Stable id; the language files name it `sc.<id>`.
    pub id: &'static str,
    pub category: Category,
    pub action: Action,
    /// The default chords — the first is the one the page shows large; the others act the same.
    pub chords: &'static [Chord],
    /// `false` = matched (the key does nothing else) but nothing acts on it yet: the page shows it
    /// dimmed.
    pub active: bool,
}
