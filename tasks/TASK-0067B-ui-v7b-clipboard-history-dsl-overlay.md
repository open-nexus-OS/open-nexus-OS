---
title: TASK-0067B UI v7b: the clipboard's two surfaces — the shell search (field + Apps / Files / Clipboard) and the keyboard's clipboard cards
status: Done (2026-10-08 — recut with the operator: the search bar of the desktop reference instead of a panel, plus the mobile reference's keyboard clipboard; board cycle 1 found three gaps, fixed and QEMU-proven; board cycle 2 confirmed `board-visual: clipboard` — operator: "noch nicht perfekt, aber ausreichend")
owner: @ui
created: 2026-03-28
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Clipboard v2 service baseline: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - SystemUI bootstrap shell: tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - DSL App Integration Kit: tasks/TASK-0122C-dsl-app-integration-kit-v1-picker-clipboard-share-print.md
  - Contract: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md (+ RFC-0075 `OP_INSERT` amendment) · Decision: docs/adr/0070-clipboardd-one-authority-focus-gated-reads-ui-in-shell-and-keyboard.md
---

## Recut 2026-10-07 — two surfaces (binding; supersedes the 2026-09-09 panel design below)

Operator decisions (2026-10-07): the clipboard lives in TWO places. (1) The top bar's right side
gets a magnifier; it opens no drop-down but ONE field that looks like the greeter's input, with
round buttons beside it — Files (inert for now), Apps (a simple filter), Clipboard — like the
desktop reference's search bar. (2) The on-screen keyboard gets the mobile reference's toolbar
(emoji · clipboard · voice · settings · more); its clipboard shows CARDS of the history instead
of the keys, a tap inserts. The clipboard is pre-filled so both surfaces show cards on every boot.

### Delivered 2026-10-07

- **Shell search** (`desktop-shell`): `topbar` magnifier pill (desktop profile, beside the
  Control Center), `components/search/{SearchOverlay,SearchModeButton,SearchResults}.nx`,
  `composables/search.store.nx`. The overlay is a shell MODAL (ESC and an outside press close
  it); the field is the greeter's recipe (panel glass, fully round, bare `TextField`) with a
  magnifier, and takes the keyboard on open (new DSL modifier `.autofocus(true)`); the three
  round buttons are panel glass (the selected one fills an accent disc, Files is dimmed and
  only absorbs its press). Results open once there is a query or a category: Apps = the
  registry's matches as tiles (a press launches and closes); Clipboard = cards, newest first
  (a press = `svc.clipboard.restore`, the card moves to the top, the header says "copied").
  Both lists are filtered AT their services; one keystroke re-asks both.
- **Keyboard** (`ime-ui`): the strip line is the toolbar while nothing composes
  (`OskToolbar` — clipboard live, the others dimmed with no press) and the candidate strip
  while the IME composes; `ClipboardCards` (the newest six, three columns) replaces the key
  rows; a card reads the FULL item (`svc.clipboard.read`) and inserts it (`svc.ime.insert` →
  imed `OP_INSERT`, RFC-0075 amendment). The page was split into `CandidateStrip`,
  `KeyRows`, `OskToolbar`, `ClipboardCards` (moved unchanged where they existed).
- **DSL**: `.autofocus(true)` (modifier id 57; the host focuses the field through the tap path
  while no field holds focus, confined to the topmost modal); grid cells that declare
  `.grow(1)` fill their track (the layout engine's grid code moved to `grid.rs`; existing grids
  unchanged — they do not grow).
- **Proof**: `tests/dsl_apps_conformance/tests/shell_search_clipboard.rs` (the injector's
  targets, the modal, autofocus, copy-back, ESC; `test_reject_*` for the inert Files button and
  the touch profiles), `ime_clipboard.rs` (toolbar has one live entry, cards, read → insert,
  strip while composing), the shell-host typing test, goldens `shell_search_buttons_{light,dark}`,
  `nexus-layout` `grid_grow_cell_fills_its_track`; QEMU `usb-visible` clipboard phase.

### Board round 2026-10-07 (operator findings → fixes)

The first board cycle showed the search but: (1) the Apps filter did not filter; (2) the
clipboard looked empty; (3) Ctrl+A / Shift+← then Ctrl+C put nothing into the clipboard, and a
copy should show up at once. Measured causes and fixes:

- **(1)** app-host's `bundlemgr.enumerate` dropped the query (the board log showed the same
  `n=` for every keystroke) → it filters by id and label now (`file_filter::app_matches`).
- **(2)** the six prefill writes reached clipboardd (`write ok` ×6, focus truth live, no deny
  line), but the Clipboard list is filtered by the typed text, and a query that matched
  nothing showed "The clipboard is empty." → a filtered empty list now says "Nothing in the
  clipboard matches this search." (`search.clipNoMatch`, five locales).
- **(3)** there was no keyboard editing at all. Built end to end:
  keymap (`KeyAction::Edit` — arrows/Home/End/Delete, Shift selects, Ctrl+A/C/X/V by the
  layout's letter, other Ctrl combinations refused) → inputd (`edit_keys`) → imed (actions
  4..=16, RFC-0075 amendment: focus-gated, a running composition commits first, copy/cut
  refused for a password field) → app-host (`probe/interaction/text_edit.rs`) → the DSL
  runtime's caret and selection on the `nexus-textedit` engine (TASK-0095's keyboard core;
  typing replaces a selection) → caret and selection painted. Copy/cut write the selection
  over the app's own `CLIPBOARD` route; paste reads the newest item; a selection over the
  item bound is stored cut short and a cut then keeps it (`item_text`). Stash and chat hold
  the `CLIPBOARD` permission now.
- **"At once"**: the copying app fires the host trigger `ClipboardChanged`; the open search
  re-lists (`on ClipboardChanged -> dispatch(ClipsReload)`). The conformance test for it
  exposed a runtime defect: the emitter's dependency walk skipped LIST OPERATIONS, so a site
  that read the list only through `len(...)`/`take(...)` never re-emitted when only the list
  changed — the panel would have stayed stale on the board. Fixed in
  `nexus-dsl-runtime` (`emit/deps.rs`, `tests/list_op_deps.rs`; the launcher pager and the
  keyboard's cards read lists the same way).
- **Typing focus**: every press in the search cleared the field's focus and autofocus handed
  it back one present later — a key typed in that gap landed nowhere. An `.autofocus(true)`
  field now holds the keyboard across presses on other controls (`tests/autofocus_hold.rs`).
- **Privacy**: clipboardd's write line no longer carries the item's length (a length per
  copy would trace typed text); app-host's copy/paste lines fire once per process.
- **Proof**: the `usb-visible` lane's clipboard step now types a word no prefill item
  contains, presses Ctrl+A, Ctrl+C, Ctrl+V and then the first card — which exists only
  because the copy re-read the history (`apphost: text copy ok`, `apphost: text paste ok`;
  budget 660 → 780 s). Host: `a_copy_in_the_open_search_refreshes_the_history_at_once`, the
  shell-host no-match check, imed/keymaps/inputd/textedit/runtime tests above.

### Closure 2026-10-08 (board cycle 2, build dev-caf749a2)

The board log shows the round's fixes at work: the app filter narrows per keystroke
(`bundlemgr.enumerate ok n=4 → 2 → 1 → 0`), a hardware Ctrl+C stored the selection
(`clipboardd: write ok (seq=9)`, `apphost: text copy ok`), card presses copied items back
(`restore ok` seq 7, 8, 10, 11; `SELFTEST: ui v7 clipboard ok`); `SELFTEST: clipboard gate
ok`, no read denial beyond the harness's own. Operator verdict: "noch nicht perfekt, aber
ausreichend" — close the task. `board-test --profile=board-visible`: ladder complete (46
rungs), acks desktop/typed/pointer/modal/clipboard on this boot; TASK-0066's `tile` rung was
not exercised on this boot (no `windowd: wm tile` line), so it was not acknowledged and the
smoke reports it missing.

### Open findings (recorded, not built)

- ~~**Privacy (follow-up first)**: `apphost: dsl svc bundlemgr.enumerate ok n=…` fires on
  EVERY keystroke of a live search.~~ **Fixed 2026-10-08** together with the whole class it
  belonged to: inputd no longer copies typed characters into the visible state
  (input-live-protocol v2, RFC-0053 amendment), and no guest line fires per keystroke —
  once-per-process proofs (`app-host/src/proof_line.rs`), change-only layer counts,
  power-of-two metric snapshots (`docs/standards/SECURITY_STANDARDS.md` §1); the 228
  per-keystroke lines in the archived board logs were scrubbed (CHANGELOG 2026-10-08).
- **Paste on the board**: no `apphost: text paste ok` on this boot (QEMU proves the path). A
  failed paste is silent today — no route, a refusal and an empty history all look the same
  to the operator; a bounded `apphost: text paste FAIL (reason=…)` line would make it
  diagnosable.
- Files search waits for an indexed file query (svc.files has listing, not search).
- The keyboard shows the newest six cards; scrolling the band is the OSK's next step.
- A copy in ANOTHER app does not refresh an already open history (e.g. the keyboard's cards):
  a live push needs its own notification channel — app-host's event inbox keeps windowd as
  its only sender (RFC-0079). Surfaces re-list on open, category change and filter.
- A caret move repaints the whole window like a keystroke does; a field-sized repaint is an
  optimization for later.
- Pointer caret placement, drag selection, word navigation and a context menu stay
  TASK-0095's.

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09:** nothing exists — no `.nx` component, page or store mentions the
clipboard; no shell entry point; no bridge (TASK-0122C is Draft); the named proof dir
`tests/ui_v7b_clipboard_history_host/` was never created. Hard dependency: TASK-0067.

### Goal (end system)

A desktop-shell panel (DSL) listing clipboardd history with MIME-aware previews and copy-back,
consuming `svc.clipboard.*` only.

### Non-goals

Any storage in the shell; sync; editing items; a second clipboard UI anywhere else.

### Invariants

Reducers pure; reads/writes are effects with `timeoutMs`; list bounded by clipboardd's
`HISTORY_MAX` (≤ 16, no paging); preview text ≤ 120 chars derived in the store, never in views.

### Decisions

- **D1 Surface** = `userspace/apps/desktop-shell/ui/components/panels/ClipboardPanel.nx`
  hosted by `PanelHost.nx` (same shape as `SoundPanel`/`WifiPanel`); store
  `ui/composables/clipboard.store.nx` (`svc.clipboard.list/restore/clear`; `OP_WATCH` events
  re-list through app-host's push intake like `WindowsChanged`).
- **D2 Entry point** = a topbar tile (`components/topbar`); dismissal via the TASK-0074
  overlay contract (`.overlay(modal)` + `onDismiss`).
- **D3 Rows** = `List` with `.key(seq)` (≤ 16 entries; no virtualization needed).
- **D4 Permission** — shell manifest `caps += "nexus.permission.CLIPBOARD"`; history reads are
  allowed because the shell is the desktop owner (0067 D3), no extra policy cap.
- **D5 Proof home** = `tests/dsl_apps_conformance` (existing); NO new proof crate.

### Packages

- **P1** store + panel + `TranscriptHost` fixtures (list / restore / empty / denied). Blast:
  `dsl_apps_conformance`, `ui_v10_goldens`.
- **P2** shell wiring + visible-lane pixel proof region. Blast: visible lane,
  `systemui_bootstrap_shell_host`.

### Definition of Done

Host: snapshot goldens light/dark (empty, 3 items, denied); interaction fixture "tap row →
`restore(seq)` dispatched → list re-ordered newest-first". QEMU: `apphost: dsl svc
clipboard.restore ok (seq=…)`, `SELFTEST: ui v7 clipboard history ok` (selftest writes 3 items,
`OP_LIST` newest-first, `OP_RESTORE` moves seq to head) + a visible-lane pixel-proof region for
the open panel (`tools/pixel_proof_judge.py`); markers registered in `ui.toml`,
`qemu-test.sh`, `markers.txt` (windowd contract only). Docs: `transfer-sharing/clipboard.md`
"History surface".

### Touched paths

`userspace/apps/desktop-shell/{manifest.toml,ui/components/panels/ClipboardPanel.nx,
ui/components/panels/PanelHost.nx,ui/composables/clipboard.store.nx,ui/components/topbar/*}`,
`tests/dsl_apps_conformance/`, markers triple.

### Dependencies

TASK-0067 (service + binding), TASK-0074 (dismissal contract).

## Context — historical, superseded by the end-state rewrite above

`TASK-0067` establishes the service and routing side of clipboard and DnD.
To make clipboard behavior actually testable and user-facing, we also need a visible clipboard history surface.

This follow-up keeps the service and the UI separate:

- `TASK-0067` owns `clipboardd`
- this task owns the visible DSL overlay/app

## Goal — historical, superseded by the end-state rewrite above

Deliver:

1. Clipboard History DSL UI:
   - overlay or small app surface rendered with the canonical DSL structure
   - list/history of recent clipboard items with MIME-aware preview
2. Integration:
   - launcher/system entry point
   - paste or copy-back action through clipboard bridge
3. Host tests + OS markers for visible clipboard behavior.

## Non-Goals

- Replacing `clipboardd`.
- Full cross-device clipboard sync.

## Constraints / invariants (hard requirements)

- UI consumes `clipboardd`; it does not reimplement clipboard storage.
- Reducers remain pure; read/write actions go through effects.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- snapshot and interaction tests for clipboard history UI

### Proof (OS/QEMU) — gated

- visible clipboard history surface opens and can restore a previous entry deterministically

## Touched paths (allowlist)

- SystemUI/launcher integration points
- clipboard DSL UI package(s)
- `tests/ui_v7b_clipboard_history_host/` (new)
- `docs/dev/ui/patterns/transfer-sharing/clipboard.md`

## Plan (small PRs)

1. DSL clipboard history UI
2. bridge integration + interactions
3. selftests + docs
