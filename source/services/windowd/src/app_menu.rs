// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Apps menu model (RFC-0065 dynamic apps menu).
//!
//! The Apps dropdown is no longer a hardcoded `const` list in the compositor — it
//! is built from the **registry** (`bundlemgrd` `OP_LIST_APPS`). This module is the
//! pure, host-tested model: it parses the registry response into entries and owns
//! the dropdown geometry. If the registry is unreachable at boot the compositor
//! falls back to [`AppMenu::seed`] (the previous Chat/Search entries) so the menu
//! never regresses.

use alloc::string::String;
use alloc::vec::Vec;

// `bundlemgrd` `OP_LIST_APPS` wire constants. The SSOT is
// `nexus_abi::bundlemgrd` (used by the os-lite server + the compositor's fetch),
// but `nexus-abi` is an os-lite-only dependency, so the host-testable parser
// mirrors the three constants it needs here. They are asserted to match in the
// compositor fetch path (which links `nexus_abi`).
const WIRE_MAGIC0: u8 = b'B';
const WIRE_MAGIC1: u8 = b'N';
const WIRE_VERSION: u8 = 1;
const WIRE_OP_LIST_APPS: u8 = 5;
const WIRE_STATUS_OK: u8 = 0;
const WIRE_BODY_OFFSET: usize = 7;

/// One app entry in the menu (id + display label), sourced from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Stable app/bundle id used to launch it (e.g. `"chat"`).
    pub id: String,
    /// Display label shown in the menu.
    pub label: String,
}

/// Max apps shown in the dropdown (bounded).
pub const MAX_MENU_APPS: usize = 12;

/// Dropdown geometry (kept in sync with the compositor's glass dropdown).
pub const DROPDOWN_PAD: u32 = 8;
pub const DROPDOWN_ROW_H: u32 = 30;

/// The dynamic Apps menu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppMenu {
    entries: Vec<AppEntry>,
}

impl AppMenu {
    /// The built-in fallback used when the registry is unreachable — the previous
    /// hardcoded dropdown contents, so boot never regresses.
    pub fn seed() -> Self {
        Self {
            entries: Vec::from([
                AppEntry { id: String::from("chat"), label: String::from("Chat") },
                AppEntry { id: String::from("search"), label: String::from("Search") },
            ]),
        }
    }

    /// Builds the menu from a `bundlemgrd` `OP_LIST_APPS` response frame. Returns
    /// `None` if the frame is malformed or empty (caller falls back to [`seed`]).
    ///
    /// Frame: `[B,N,ver,OP_LIST_APPS|0x80, status, count:u16le,
    ///          (id_len:u8,id,label_len:u8,label)*]`.
    pub fn from_list_apps_response(frame: &[u8]) -> Option<Self> {
        if frame.len() < WIRE_BODY_OFFSET
            || frame[0] != WIRE_MAGIC0
            || frame[1] != WIRE_MAGIC1
            || frame[2] != WIRE_VERSION
            || frame[3] != (WIRE_OP_LIST_APPS | 0x80)
        {
            return None;
        }
        let status = frame[4];
        let count = u16::from_le_bytes([frame[5], frame[6]]);
        if status != WIRE_STATUS_OK || count == 0 {
            return None;
        }
        let mut pos = WIRE_BODY_OFFSET;
        let mut entries = Vec::new();
        for _ in 0..count {
            let id = take_lp_str(frame, &mut pos)?;
            let label = take_lp_str(frame, &mut pos)?;
            entries.push(AppEntry { id, label });
            if entries.len() >= MAX_MENU_APPS {
                break;
            }
        }
        if entries.is_empty() {
            return None;
        }
        Some(Self { entries })
    }

    /// Menu entries (back to front in the dropdown).
    pub fn entries(&self) -> &[AppEntry] {
        &self.entries
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Open height of the dropdown for the current entry count.
    pub fn dropdown_full_h(&self) -> u32 {
        DROPDOWN_PAD * 2 + DROPDOWN_ROW_H * self.entries.len() as u32
    }

    /// Which entry index a dropdown-local `local_y` falls in.
    pub fn item_at(&self, local_y: u32) -> Option<usize> {
        for i in 0..self.entries.len() {
            let top = DROPDOWN_PAD + i as u32 * DROPDOWN_ROW_H;
            if local_y >= top && local_y < top + DROPDOWN_ROW_H {
                return Some(i);
            }
        }
        None
    }

    /// The app id at a dropdown row (for launch dispatch).
    pub fn id_at(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|e| e.id.as_str())
    }
}

/// Reads a `[len:u8, bytes...]` UTF-8 string at `*pos`, advancing `*pos`.
fn take_lp_str(frame: &[u8], pos: &mut usize) -> Option<String> {
    let len = *frame.get(*pos)? as usize;
    let start = *pos + 1;
    let body = frame.get(start..start + len)?;
    let s = core::str::from_utf8(body).ok()?;
    *pos = start + len;
    Some(String::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a LIST_APPS response like bundlemgrd's os-lite handler.
    fn response(apps: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(WIRE_MAGIC0);
        out.push(WIRE_MAGIC1);
        out.push(WIRE_VERSION);
        out.push(WIRE_OP_LIST_APPS | 0x80);
        out.push(WIRE_STATUS_OK);
        out.extend_from_slice(&(apps.len() as u16).to_le_bytes());
        for (id, label) in apps {
            out.push(id.len() as u8);
            out.extend_from_slice(id.as_bytes());
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
        out
    }

    #[test]
    fn parses_registry_response() {
        let frame = response(&[("chat", "Chat"), ("search", "Search"), ("notes", "Notes")]);
        let menu = AppMenu::from_list_apps_response(&frame).expect("parse");
        assert_eq!(menu.len(), 3);
        assert_eq!(menu.id_at(0), Some("chat"));
        assert_eq!(menu.entries()[2].label, "Notes");
    }

    #[test]
    fn item_at_maps_rows() {
        let frame = response(&[("chat", "Chat"), ("notes", "Notes")]);
        let menu = AppMenu::from_list_apps_response(&frame).unwrap();
        assert_eq!(menu.item_at(DROPDOWN_PAD + 2), Some(0));
        assert_eq!(menu.item_at(DROPDOWN_PAD + DROPDOWN_ROW_H + 2), Some(1));
        assert_eq!(menu.item_at(9999), None);
        assert_eq!(menu.dropdown_full_h(), DROPDOWN_PAD * 2 + DROPDOWN_ROW_H * 2);
    }

    #[test]
    fn malformed_or_empty_falls_back_to_none() {
        assert!(AppMenu::from_list_apps_response(&[]).is_none());
        // status OK but count 0.
        let empty = response(&[]);
        assert!(AppMenu::from_list_apps_response(&empty).is_none());
        // truncated entry.
        let mut bad = response(&[("chat", "Chat")]);
        bad.truncate(8);
        assert!(AppMenu::from_list_apps_response(&bad).is_none());
    }

    #[test]
    fn seed_is_chat_and_search() {
        let seed = AppMenu::seed();
        assert_eq!(seed.len(), 2);
        assert_eq!(seed.id_at(0), Some("chat"));
        assert_eq!(seed.id_at(1), Some("search"));
    }

    #[test]
    fn entry_count_is_bounded() {
        let many: Vec<(&str, &str)> = (0..MAX_MENU_APPS + 5).map(|_| ("app", "App")).collect();
        let frame = response(&many);
        let menu = AppMenu::from_list_apps_response(&frame).unwrap();
        assert_eq!(menu.len(), MAX_MENU_APPS);
    }
}
