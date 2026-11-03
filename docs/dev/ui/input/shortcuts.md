<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Shortcuts

Shortcuts are additive “power features” and must not be the only way to discover actions:
expose primary actions in the UI, keep the shortcut set small and consistent.

## One registry (RFC-0113 §7, TASK-0346)

Every key chord the system acts on lives in ONE table, `userspace/shortcuts/src/table.rs`, and is
matched and listed from there — never a second copy in a service, a language file or a document.

- **An entry:** `id` (`window.fill`), `category` (windows, screenshots, text, input), `action`
  (its owner follows: windowd, the shell, the focused field, inputd), `chords` (the first is the
  primary, alternates act the same), `active` (false = matched, nothing acts on it yet — listed
  dimmed).
- **A chord** is an EXACT modifier set (Shift, Ctrl, Alt, AltGr, Super — the surface wire's bits)
  and one key: by its place (the arrows, Home, Print) or by the letter the active layout types on
  it (Ctrl+Z is the key that types `z` — on a German keyboard the one labelled Z; a layout without
  a Latin letter there takes the key's US letter). One more or one fewer modifier is another
  chord: Super+Ctrl+Shift+F is no Fill.
- **Matched in one place each:** inputd takes the window and capture chords before a key enters
  the text route (`shortcuts::system_action` — events on the key channel, `KEY_INPUT_CHORD` /
  `KEY_INPUT_CAPTURE`, TASK-0348 P17); the keymap resolves the field commands, the focus moves
  (Tab → `text.focusNext`, Shift+Tab → `text.focusPrevious`) and the input-source switch
  (`shortcuts::lookup`). The plain keys a layout produces (Enter, Backspace, Escape) are layout
  data; the keymap tables carry no Tab row.
- **Listed:** Settings › Allgemeine Verwaltung › Eingabe › Tastaturkurzbefehle opens a window
  (DSL, app-modal over the settings window) — categories left, the category's chords right
  (`svc.shortcuts.list(category, locale)`, key caps German for `de*`, Latin otherwise), closed by
  Fertig, ESC or the backdrop. Rebinding is drawn, disabled — a later phase.

## Adding a shortcut

1. A row in `table.rs` (the matcher of its owner picks it up — no other code).
2. Its name `sc.<id>` in the settings app's five language files.
3. Its arm in `userspace/apps/settings/ui/components/shortcuts/ScName.nx` (`@t` takes literal
   keys).

The window (`ShortcutsWindow.nx`) lives on the settings page's root like its picker sheets — a
full-window layer belongs to the page that owns the z-order, and it keeps the scene shallow.

`cargo test -p shortcuts` refuses a chord two entries share, a match that is not exact, and an id
without its name in every language or its arm on the page; the keymap's contract test resolves
every field chord in every layout; windowd's tests pin the window codes to its zones.
