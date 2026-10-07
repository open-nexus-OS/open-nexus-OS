<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Clipboard

One authority holds the clipboard: `clipboardd` (RFC-0094, ADR-0070). Every surface is a
client — no surface keeps its own copy.

## Model (v1: text)

- A bounded history, newest first (16 items, ≤ 248 bytes each). Copying a text that is
  already there moves it to the top; a full history drops its oldest item.
- Every item carries a monotonic `seq` (the handle for read and copy-back), the writer's
  kernel id and its origin device (0 = this device — the field a later sync fills).
- Ephemeral: nothing persists across a reboot or a restart of the service.

## Who may do what

| Action | Who |
|---|---|
| Copy (write) | any app holding `nexus.permission.CLIPBOARD`, or a service with a declared route |
| Paste (read one item) | the focused window, the shell, the keyboard |
| Browse, copy back, clear (the history) | the shell and the keyboard only |

The compositor tells the clipboard who the focused window, the shell and the keyboard are
(kernel ids, pushed on every change). A page never sends identity: the kernel attributes
every request.

## The two surfaces

- **Desktop — the search** (top bar magnifier): one field like the greeter's, round buttons
  for Apps, Files (not yet) and Clipboard. Clipboard shows the history as cards; pressing a
  card copies it back (it becomes the newest). Typing filters the history and the apps at
  their services.
- **Touch — the keyboard**: the toolbar above the keys (emoji, clipboard, voice, settings,
  more — only the clipboard is live today). The clipboard replaces the keys with cards of the
  newest six items; pressing a card inserts its full text into the focused field.

## In every text field (keyboard)

Every DSL text field edits like a desktop field: arrows, Home/End and Delete move and delete,
Shift with them selects, Ctrl+A selects everything, and typing replaces a selection. Ctrl+C
copies the selection into the history, Ctrl+X cuts it, Ctrl+V pastes the newest item at the
caret. Shortcuts follow the letter the active layout puts on the key. A password field never
copies or cuts. A selection longer than one item (248 bytes) is copied cut short, and a cut
then keeps the text in the field. The app needs `nexus.permission.CLIPBOARD`.

A page that shows the history refreshes after a copy made in the same app:

```nx
on ClipboardChanged -> dispatch(ClipsReload)   // the host fires it after this app's own copy
```

Copies in other apps are seen the next time the surface lists: on open, on a category
change, on a new filter.

## In a page (`svc.clipboard.*`)

```nx
@effect on ClipsLoad {
    match svc.clipboard.list(state.query) {   // newest first, filtered at the service
        Ok(items) => dispatch(ClipsLoaded(items)),   // List<ClipEntry { seq, text }>
        Err(e) => dispatch(ClipsFailed(e)),          // ERR_SVC_DENIED off the shell/keyboard
    }
}
```

`read(seq)` returns one item's full text (`0` = the newest), `write(text)` copies,
`restore(seq)` copies back, `clear()` empties. Previews in a list are ≤ 120 bytes.

## Not yet

Rich flavors (HTML, images) and large items (TASK-0087); drag and drop (TASK-0086); sync with
the same account's other devices; a live refresh of an open history after another app's copy;
pointer selection, word navigation and a context menu in text fields (TASK-0095).
