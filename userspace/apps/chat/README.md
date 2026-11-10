# chat — DSL app

The chat app, rebuilt to the design artifacts' main screen (the Chats view with a conversation
open and Chat-Info shown): the window kit's chrome and app menu, a space rail and a navigation
sidebar with the profile strip, the content pane — the conversation list (title, search, filter
chips) beside the thread (header, transcript, composer) — and the Chat-Info pane. The transcript
is the window's scroll layer, which the compositor moves (RFC-0109), and it pages older messages
through the platform's query engine (keyset paging) as it scrolls; the store keeps only the last
64 messages resident, so a long transcript costs a bounded frame.

The conversations are AUTHORED rows (names, last lines, transports, flags): there is no contacts,
threads or transport service yet, so the data is demo data and every transport, route,
verification and call control is shown dimmed and inert (unfinished controls are disabled, not
deleted).

## Routes

One page: `"/" -> ChatPage` (`ui/pages/Routes.nx`, `ui/pages/ChatPage.nx`). The list and the
thread are one screen: on the wide tier side by side, on the regular and compact tiers they take
turns (opening a row pushes the thread, its back button returns).

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `ChatStore` (`ui/composables/chat.store.nx`) | the resident messages (`{seq, text}`, the last 64), the local echoes, the draft, the paging token, loading and the last error | `ChatEvent`: `LoadFirst`, `LoadMore`, `Loaded(rows, next)`, `LoadFailed`, `Send`, `ClearDraft`, `Received` |
| `ThreadStore` (`ui/composables/thread.store.nx`) | the open conversation: id, name, initials, address, up to two transports with their tint roles, the verified / bridged / merged flags, the space, the route and the compact tier's push state | `ThreadEvent`: `Init` (root effect: opens the first row), `OpenThread(…)`, `SetRoute`, `BackToList` |
| `ListStore` (`ui/composables/list.store.nx`) | the search text, the filter chip and the space picked on the rail | `ListEvent`: `QueryChanged`, `ClearQuery`, `SetChip`, `SetSpace` |
| `WindowStore` (window-kit's `ui/composables/window.store.nx`, compiled in with the chrome) | the window kit's chrome state: the open menu, the panes | the window kit's `Win*` events (RFC-0084) |

Who wrote a message is the sequence number's parity (even = you), never parsed from its text.
Chat-Info has one state, `WindowStore.rightOpen`: the chrome's toggle, the thread header's Info
button and the menu's "Chat-Info ausblenden / einblenden" all flip it. Opening a conversation
resets the route and clears the draft (an effect bridges `ThreadStore` to `ChatStore`).

## Services

| Call | When |
|---|---|
| `Transcript(token)` — `Query Transcript on messages { orderBy seq, limit 60 }` | `LoadFirst` and `LoadMore` (the scroll layer's `EndReached`): the next 60 rows and the next token |
| `svc.settings.set("window.control", …)` | the window controls, through the window kit |

## Window

`Window { style: plain, mode: freeform, resizable: true, width: 1240, height: 780, minWidth: 360,
minHeight: 600 }` — the window opens at the design's size and is never smaller than a phone
(RFC-0117). The window kit's chrome sits inside the app's own surface, the zones in opaque panes
([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md)). Size tiers are the kit's
(`device.sizeClass`, 640 / 1024): wide shows rail, sidebar, list, thread and Chat-Info inline;
regular turns the sidebar into an overlay and list and thread into a push; compact also makes
Chat-Info an overlay.

## Structure

```
ui/pages/ChatPage.nx                  the window: the kit's top bar and body, one zone per slot
ui/components/rail/SpaceRail, RailTab the space rail (a column on wide, a row in the overlay) and its tabs
ui/components/sidebar/                SidebarZone (rail + sidebar), ChatSidebar (the views), NavRow, ProfileFooter
ui/components/list/                   ListColumn: ListHeader (title, search, FilterChips), ConversationList
                                      (the authored rows), ConversationRow (filters itself, selected layer, tap), RowFace
ui/components/thread/                 ContentPane and ChatSplit (list | thread, or a push), ThreadPane,
                                      ThreadHeader with ThreadBadges and ThreadActions, Transcript with
                                      EncryptionPill and MessageBlock
ui/components/composer/               Composer (field, Send) and RouteRow (route chips, fallback note)
ui/components/info/                   InfoPane (identity, actions, note), InfoFacts and InfoRow (the key-value group)
ui/components/menu/ChatMenu           the ⋮ app menu: Datei ▸, Ansicht ▸, Einstellungen
ui/composables/chat.store.nx          the transcript, the local echoes, the draft, the paging query
ui/composables/thread.store.nx        the open conversation
ui/composables/list.store.nx          the search text, the chip, the space
i18n/de.json, en.json, ja.json, ko.json, zh.json
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `chat.rs` (the transcript fed by the real query engine:
first page, paging on `EndReached`, the resident window, the transcript as the rightmost scroll
layer), `chat_screen.rs` (the first conversation open, the chips and the space narrowing the
list, opening a row, an empty draft, the compact push and back), `frame_arena_budget.rs` (one
frame's arena cost, the largest of the apps), `budget_probe.rs` and `apps_compile.rs`. Run them with
`cargo test -p dsl_apps_conformance --test chat --test chat_screen --test frame_arena_budget`.

## Known limits

- Demo data: the nine conversations, their transports and flags, the profile and the navigation
  counts are authored (the DSL has no record literal to loop over, so each row is one component
  with its data inline). Every thread shows the same paged transcript; sending appends a local
  echo and clears the draft — messages stay local until the app-to-app transport lands (the
  manifest keeps the `chat.Send`/`chat.Receive` exports declared for it). An empty draft sends
  nothing.
- Inert by design (shown dimmed): the other views, the join-a-space tab, voice and video calls,
  the mini app, attachments, the route chips, "Anrufen" / "Bezahlen", the file menu and the
  settings entry. There is no presence data, so no online dot and no "online" status line (the
  second header line is the address).
- The search matches a conversation's name exactly (there is no substring test in the DSL), the
  count beside the title is the authored total, and there is no "no results" state yet.
- Text never wraps in the DSL: long notes are set as two lines, names and last lines are cut with
  an ellipsis, and the filter chips fit one line only at the list's width (no flex wrap).
- The transcript is top-anchored (no scroll-to-end anchor), shows no per-message transport badge
  or time (the host table is `(seq, text)`), and has neither app-card messages nor system notes.
- Chat-Info starts hidden: the kit's window store starts with `rightOpen` 0. The toolbar shows
  four tools: new chat, search (live), call and mini-apps (drawn disabled until their functions
  exist); there is no "add account" button (no trailing slot).
