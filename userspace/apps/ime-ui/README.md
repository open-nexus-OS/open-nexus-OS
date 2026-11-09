# ime-ui — DSL app

The on-screen keyboard (RFC-0075 Phase 8b; design artifacts: OnScreenKeyboard, docked): a
chromeless overlay that windowd shows on text focus and docks to the bottom display edge. One
opaque tray holds the toolbar (language chip · candidates · emoji · clipboard · voice · settings
· more · close) and under it exactly one body — the keys or the clipboard's cards — at the same
height, so the keyboard never jumps. Its taps reach imed's dedicated keyboard endpoint
(`svc.ime.*`); the `ime` bundle type is the privilege ceiling for `nexus.permission.IME`, so a
normal app can never hold that route. Not user-launchable.

## Routes

One page: `"/" -> OskPage` (`ui/pages/Routes.nx`, `ui/pages/OskPage.nx`).

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `OskStore` (`ui/composables/osk.store.nx`) | the layout tag, the four key rows (`{label, key, action}` records from the keymaps SSOT), the mirrored preedit and the eight candidates | `OskEvent`: `Key`, `Action`, `CycleLayout`, `Dismiss`, `CommitDismiss`, `CommitCycle`, `Select`, `Sent`, `SendFailed`, `Rows0`–`Rows3` · `KeymapEvent`: `Changed` · `ImeStripEvent`: `Preedit`, `Cands` |
| `OskClipStore` (`ui/composables/clipboard.store.nx`) | which body shows (`oskPanel`: keys or `"clipboard"`), the history's cards, whether clipboardd refused the list | `OskClipEvent`: `ClipToggle`, `ClipCommitOpen`, `OskClips`, `OskClipsFailed`, `ClipPaste`, `ClipText`, `ClipPasteFailed`, `ClipClear`, `ClipCleared`, `ClipClearFailed` |

The rows are data: windowd pushes `KeymapEvent::Changed` when `input.keymap` changes and the store
reloads the four rows — a new language is a new data table, never a branch here. A running
composition is committed first (`svc.ime.action("enter")`) before the keyboard closes, the
layout switches or the clipboard opens: imed resets the engine on a switch, so the typed text
would otherwise be lost. A key goes to imed and nowhere else; nothing logs one.

## Services

| Call | When |
|---|---|
| `svc.ime.rows(tag, row)` | on `KeymapEvent::Changed`: the layout's rows 0–3 (row 0 = digits) |
| `svc.ime.key(key)` | a character key, space and "." |
| `svc.ime.action(name)` | an action key (⌫, return); `"enter"` also commits a composition |
| `svc.ime.select(index)` | a candidate's tap |
| `svc.ime.cycle(tag)` | the globe key: the next system layout (imed persists `input.keymap`; the change comes back as `KeymapEvent`) |
| `svc.ime.insert(text)` | a clipboard card's text, committed like typed text |
| `svc.clipboard.list("")`, `read(seq)`, `clear()` | the cards: the history, a card's full text, "Alle löschen" |
| `svc.settings.set("window.control", "minimize")` | the close key hides the keyboard |

## Window

`Window { style: plain, level: overlay, resizable: false }` — windowd docks the overlay at the
band height it owns (312 px today: a 36 px toolbar and five rows of 44 px); the keyboard decides
nothing about its own placement ([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md)).

## Structure

```
ui/pages/OskPage.nx                     the tray: the toolbar, then the keys or the cards
ui/components/toolbar/OskToolbar        chip · candidates · ghost keys (clipboard and close live)
ui/components/toolbar/CandidateStrip    up to eight candidates while composing; a tap commits one
ui/components/toolbar/OskGhostKey       a toolbar icon button's face
ui/components/keys/KeyRows              the key area: number row, three letter rows, bottom row
ui/components/keys/KeyRow               one row of character keys
ui/components/keys/ShiftRow             the last letter row: Shift · letters · ⌫
ui/components/keys/RowCell              a cell of that row, by its data: character or action key
ui/components/keys/BottomRow            123 · globe · space · . · return (no "." on compact)
ui/components/keys/OskFnIcon            a function key's icon face
ui/components/clipboard/ClipboardCards  the clipboard in place of the keys: header, cards, panel bar
ui/components/clipboard/ClipCard        one card; a tap on its body pastes it
ui/composables/osk.store.nx             the rows, the strip and the calls to imed
ui/composables/clipboard.store.nx       the clipboard body and the calls to clipboardd
i18n/de.json, en.json, ja.json, ko.json, zh.json
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `ime_keys.rs` (with real rows: every key shows its label
and sends its key, ⌫ sends its action and never an empty key, Shift and ⌫ stay one and a half
keys wide for 7 and for 8 letters) and `ime_clipboard.rs` (the toolbar, the cards, a paste reads
the full text, a composition is committed first, unbacked controls take no press). Run them with
`cargo test -p dsl_apps_conformance --test ime_keys --test ime_clipboard`.

## Known limits

Shift and "123" are drawn disabled: the rows carry no shifted labels and no symbol pages (spec
gap G5). Emoji (no colour-emoji glyphs are baked), voice, settings and more take no press. The
candidate strip stays empty while nothing composes — a prediction bar needs a prediction source
in imed (G1). A card's delete button is disabled: clipboardd has no delete-one, no pin and no
timestamps (G8), and a card shows one line until text wraps. The space and return names follow
the UI language, not the keyboard's layout (G6).
