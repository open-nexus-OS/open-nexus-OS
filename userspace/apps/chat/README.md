# chat — DSL app

The chat app: an overview of the open conversations, and the transcript of the one picked —
sender-aligned bubbles (yours right on `accent`, the peer's left on `fill`), the contact's name
and status above, the composer below. The transcript is the window's scroll layer, which the
compositor moves (RFC-0109), and it pages older messages through the platform's query engine
(keyset paging) as it scrolls; the store keeps only the last 64 messages resident, so a long
transcript costs a bounded frame.

## Routes

One page: `"/" -> ChatPage` (`ui/pages/Routes.nx`, `ui/pages/ChatPage.nx`). The overview and the
transcript are a branch on the open thread, not two routes.

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `ChatStore` (`ui/composables/chat.store.nx`) | the resident messages (`{seq, text}`, the last 64), the draft, the paging token, loading and the last error, the open thread and its contact's name | `ChatEvent`: `OpenThread(id, name)`, `CloseThread`, `LoadFirst`, `LoadMore`, `Loaded(rows, next)`, `LoadFailed`, `Send`, `Received` |
| `WindowStore` (`ui/composables/window.store.nx`) | the window kit's chrome state: the open menu, the panes | the window kit's `Win*` events (RFC-0084) |

The sender is derived from the message's sequence number (even = you, odd = the peer), never
parsed from its text.

## Services

| Call | When |
|---|---|
| `Transcript(token)` — `Query Transcript on messages { orderBy seq, limit 60 }` | `LoadFirst` and `LoadMore` (the scroll layer's `EndReached`): the next 60 rows and the next token |
| `svc.settings.set("window.control", …)` | the window controls, through the window kit |

## Window

`Window { style: plain, mode: freeform, resizable: true }` — the window kit's chrome inside the
app's own surface, the transcript in an opaque content pane
([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md)).

## Structure

```
ui/pages/ChatPage.nx               the window: the kit's top bar, the pane with header, body, composer
ui/components/chat/ChatHeader      the title, or the open thread's back button, avatar, name, status
ui/components/chat/ChatOverview    the conversations (one ChatListRow each) while no thread is open
ui/components/chat/ChatListRow     one conversation: avatar, name, last line, time, unread badge
ui/components/chat/ChatTranscript  the scroll layer of message rows; its end loads the next page
ui/components/chat/ChatBubble      one message: yours on `accent`, the peer's on `fill`
ui/components/chat/ChatComposer    the message field and Send while a thread is open
ui/composables/chat.store.nx       the transcript, the draft, the paging query
ui/composables/window.store.nx     the window chrome state
i18n/de.json, en.json, ja.json, ko.json, zh.json
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `chat.rs` (the transcript fed by the real query engine:
first page, paging on `EndReached`, the resident window), `frame_arena_budget.rs` (one frame's
arena cost, the largest of the apps), `budget_probe.rs` and `apps_compile.rs`. Run them with
`cargo test -p dsl_apps_conformance --test chat --test frame_arena_budget`.

## Known limits

The four conversations of the overview are authored rows: there is no contacts or threads
service yet, so every thread opens the same transcript. Sending clears the draft and nothing
else — messages stay local until the app-to-app transport lands (the manifest keeps the
`chat.Send`/`chat.Receive` exports declared for it). The window store is a copy of the kit's
contract until a library may ship one (TASK-0357 P4). The design pass for this app is open
(TASK-0353 P3).
