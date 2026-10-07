# RFC-0094: Content transfer v1 — `clipboardd`, the one clipboard authority

- Status: In Progress (2026-10-07 — Phases 0–2 implemented; board proof pending)
- Owners: @ui @runtime
- Created: 2026-10-07
- Last Updated: 2026-10-07
- Links:
  - Tasks: `tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md` (the service, the gate, the wire),
    `tasks/TASK-0067B-ui-v7b-clipboard-history-dsl-overlay.md` (the two surfaces)
  - ADRs: `docs/adr/0070-clipboardd-one-authority-focus-gated-reads-ui-in-shell-and-keyboard.md`
    (supersedes ADR-0008)
  - Related RFCs: RFC-0075 (IME v2 — the `OP_INSERT` amendment), RFC-0086 (window feed —
    the owner sids), RFC-0093 (routing v2 — declared slots), ADR-0068 (modal semantics)

## Status at a Glance

- **Phase 0 (contract + wire)**: ✅ `nexus_wire::clipboardd` (`'C','B'` v1), this RFC, ADR-0070
- **Phase 1 (authority)**: ✅ `clipboardd` — history, gate, answer; host contract tests
- **Phase 2 (surfaces)**: ✅ windowd focus truth, `svc.clipboard.*`, the shell search and the
  keyboard's clipboard; QEMU proof on the `usb-visible` lane
- **Phase 3 (flavors)**: ⬜ TASK-0087 — html/rtf/image flavors, the VMO path, a background-write rule
- **Phase 4 (drag and drop)**: ⬜ TASK-0086 — DnD routing in windowd, one-shot transfer items
- **Phase 5 (sync)**: ⬜ a dsoftbus consumer of the same history (same account, other devices)

## Scope boundaries (anti-drift)

- **This RFC owns**: the clipboard wire, the authority's gate (who may write, read, browse),
  the focus-truth push, the history's bounds and ordering, and the item shape every later
  phase extends (`seq`, writer sid, origin device).
- **This RFC does NOT own**: drag and drop (TASK-0086 owns routing and transfer items),
  non-text flavors and format conversion (TASK-0087), persistence (the clipboard is ephemeral
  by contract), cross-device sync (a later dsoftbus consumer), the UI (TASK-0067B, ADR-0070).

### Relationship to tasks (single execution truth)

TASK-0067 proves Phases 0–1 and the windowd half of Phase 2; TASK-0067B proves the surfaces.

## Context

The tree carried a 14-line placeholder (`clipboardd` printing `ready` with nothing behind it)
and a host-only `Mutex<Option<String>>` library (ADR-0008). Nothing could copy, nothing could
paste, and no surface could show what was copied. The operator asked for the clipboard in two
places — the desktop shell's search (the reference desktop's search bar with an Apps, a Files
and a Clipboard button) and the on-screen keyboard (the mobile reference's toolbar entry that
swaps the keys for cards) — text only at first, with cross-device sync kept in view.

## Goals

- ONE clipboard authority; every surface is a client of it.
- Identity from the kernel, never from a payload: the gate compares the request's
  `sender_service_id` with owners windowd names.
- A bounded history (newest first) the shell and the keyboard can browse and copy back from.
- An item shape that already carries what sync and flavors will need.

## Non-Goals

Drag and drop; flavors other than `text/plain`; persistence across boots; sync; editing items.

## Constraints / invariants (hard requirements)

- **Identity** = the kernel's `sender_service_id`. Sender id 0 is nobody.
- **Deny-by-default**: before windowd's first focus push no owner is known, every read refused.
- **Bounds**: text ≤ 248 bytes (inline v1), preview ≤ 120 bytes, query ≤ 64 bytes,
  history ≤ 16 items; every decoder exact-length and fail-closed.
- **No contents in logs**: markers carry `seq`, lengths and counts only.
- **No heap churn**: the history is fixed storage (the os-lite allocator never frees).
- **A focus push is never answered**, and no reply is a blocking send without a moved cap.

## Proposed design

### Contract / interface (normative)

Envelope `'C','B'`, version 1. Replies set `op | 0x80`.

| Op | Request | Reply | Access |
|---|---|---|---|
| `OP_WRITE = 1` | `text_len:u8, text` | `status, seq:u64` | Write |
| `OP_READ = 2` | `seq:u64` (0 = newest) | `status, seq:u64, text_len:u8, text` | Read |
| `OP_LIST = 3` | `query_len:u8, query` | `status, count:u8, packed_len:u16, packed` | History |
| `OP_RESTORE = 4` | `seq:u64` | `status, new_seq:u64` | History |
| `OP_CLEAR = 5` | `reserved:u8` | `status` | History |
| `OP_FOCUS = 6` | `focused:u64, desktop:u64, ime:u64` | none | windowd only |

A packed list entry is `seq:u64, writer_sid:u64, device:u64, len:u8, preview` — newest first,
filtered AT the service (`query` = ASCII-case-insensitive substring, "" = all; other bytes
compare exactly). Status: `OK 0, MALFORMED 1, DENIED 2, EMPTY 3, UNSUPPORTED 4`.

**The gate** (`clipboardd::gate`):

- **Write** — every route holder. The route is the capability: an app holds it only with
  `nexus.permission.CLIPBOARD` in its manifest (`nexus-sdk-routes`, child slot 23), a service
  only by a declared topology edge. This is the mobile reference's background-write model; a
  stricter background rule belongs to Phase 3.
- **Read** (the paste) — the focused window's owner, the shell (desktop owner) and the
  keyboard (IME owner).
- **History** (list, restore, clear) — the shell and the keyboard only: an app pastes the
  newest item, it never browses what others copied.
- **Focus** — windowd's kernel sid alone; anyone else's push is dropped and logged (bounded).

**The history**: writing a text already present moves it to the top under a new `seq` (one
copy); a full history drops its oldest item; restore re-stamps (newest = largest `seq` always);
a `seq` is never reused, also across `clear`.

**Focus truth** (windowd → clipboardd): windowd computes `(focused owner, desktop owner, IME
overlay owner)` on its frame pass — the focused window (or the topmost before any raise), the
desktop-surface owner captured at create, the live `level: overlay` slot's owner — and pushes
the triple when it changed or a send is owed (NONBLOCK, deduped, retried next pass). There is no
focus-change call site to forget: the pass is the push.

**IME insert** (RFC-0075 amendment): `OP_INSERT = 9` on imed's dedicated OSK endpoint commits a
given text into the focused field as one commit (≤ 64 bytes per frame; app-host cuts longer
text into consecutive pieces on char boundaries and sends them kernel-parked, so none is
dropped). The main endpoint refuses the op.

**Copy, cut and paste in text fields** (TASK-0067B board round; keys per the RFC-0075
editing-actions amendment). The focused app owns its selection: Ctrl+C writes the selected
text with `OP_WRITE` over the app's own route (the Write access above — no new op), Ctrl+X
does the same and then removes the selection, Ctrl+V reads the newest item (`OP_READ`,
`seq = 0`; the Read access — the app's window holds focus) and inserts it at the caret. An
item holds at most `TEXT_MAX_BYTES`: a longer selection is stored cut on a char boundary
(`nexus_wire::clipboardd::item_text`), and a cut then KEEPS the selection — the clipboard
does not hold it whole, removing it would lose text. A password field never yields its text
(imed refuses `COPY`/`CUT` there, and the DSL runtime returns no selection for a secure field).

**Freshness**: after its own successful write the app fires the host trigger
`ClipboardChanged`; a page that shows the history declares `on ClipboardChanged ->
dispatch(…)` and re-lists, so the open shell search shows a copy made in its own field at
once. Other surfaces re-list when they open, change category or filter. A live push of
another app's copy into an open history needs its own notification channel: app-host's event
inbox has windowd as its only sender (the RFC-0079 window-close contract), so clipboardd
cannot write into it — recorded as an open question below.

### Phases / milestones (contract-level)

See "Status at a Glance". Later phases are append-only: new ops, new statuses, new fields at
the end of a list entry — never a change to v1 frames.

## Security considerations

- **Threat**: an app reads what another app copied (passwords, codes). **Mitigation**: reads
  need focus by kernel identity; the history is the shell's and the keyboard's only.
- **Threat**: a forged focus push widens the gate. **Mitigation**: only windowd's kernel sid
  may push; the payload names owners but cannot name its sender.
- **Threat**: a background app replaces the clipboard (address swapping). **Accepted in v1**,
  as on the mobile reference; the previous item stays in the history; Phase 3 adds a policy.
- **Threat**: log leakage. **Mitigation**: markers carry sequence numbers and counts only —
  no contents and no content lengths (a length per copy would trace what the user typed);
  app-host's copy and paste lines fire once per process. Host-tested
  (`markers_name_numbers_never_contents`).
- **Threat**: a password leaves its field through the clipboard. **Mitigation**: imed refuses
  `COPY`/`CUT` for a password focus (`test_reject_copy_and_cut_out_of_a_password_field` in
  imed), and the runtime yields no selection for a secure field (the same-named test in
  `nexus-dsl-runtime/tests/text_editing.rs`).
- **Proof**: `clipboardd/tests/contract.rs` — `test_reject_reads_before_windowd_names_any_owner`,
  `test_reject_read_from_an_unfocused_app`, `test_reject_history_for_a_focused_app`,
  `test_reject_focus_push_from_anyone_but_windowd`, `test_reject_sender_zero_and_malformed_frames`,
  `test_reject_unknown_seq`; the wire's `test_reject_bounds_truncation_and_foreign_frames`.

## Failure model (normative)

- clipboardd down: `svc.clipboard.*` answers `ERR_SVC_UNAVAILABLE`; the surfaces show
  "not available"; supervision restarts it (`Standard`, `OnFailure`) with an empty history —
  ephemeral by contract — and windowd's next push re-arms the gate.
- A full queue toward clipboardd: windowd keeps the push owed and retries; nothing blocks.
- A malformed request: `MALFORMED`, nothing stored.

## Proof / validation strategy (required)

### Proof (Host)

`cargo test -p clipboardd -p nexus-wire -p imed`; the shell search and keyboard conformance
(`tests/dsl_apps_conformance/tests/{shell_search_clipboard,ime_clipboard}.rs` — incl.
`a_copy_in_the_open_search_refreshes_the_history_at_once`, the lane's copy step at the
layout), the shell-host typing test, the search-button goldens; the editing core
(`nexus-textedit`, `nexus-dsl-runtime/tests/{text_editing,autofocus_hold,list_op_deps}.rs`,
the keymaps contract, inputd's edit-key map).

### Proof (OS/QEMU)

Every lane: `SELFTEST: clipboard gate ok` (the deny side + the history's six-item prefill),
`SELFTEST: ime insert ok`. `usb-visible`: the injector opens the search, picks Clipboard,
types a word no prefill item contains (no card), presses Ctrl+A, Ctrl+C (the history
re-reads at once), Ctrl+V, and presses the first card — which exists only because of that
re-read — `clipboardd: focus truth live`, `apphost: text copy ok`, `apphost: text paste ok`,
`apphost: dsl svc clipboard.restore ok (seq=…)`, `SELFTEST: ui v7 clipboard ok` (chain group
`clipboard`). Board: the operator rung `board-visual: clipboard`.

### Deterministic markers (if applicable)

`clipboardd: ready` · `clipboardd: write ok (seq=…)` · `clipboardd: {read,list,restore,clear}
deny (reason=clipboard-focus)` · `clipboardd: focus truth live` · `clipboardd: restore ok (seq=…)`
· `SELFTEST: ui v7 clipboard ok` · `apphost: dsl svc clipboard.restore ok (seq=…)` ·
`apphost: dsl svc ime.insert ok` · `apphost: text copy ok` · `apphost: text paste ok` (once
per process each).

## Alternatives considered

- **clipboardd asks windowd who is focused** — rejected: a call per read, a dependency cycle
  risk, and a race between the ask and the read; a pushed, retained truth has neither.
- **The shell holds the history** — rejected: the keyboard needs the same history, and a
  second store is a second truth (no dual structure).
- **The list carries full texts** — rejected: sixteen 248-byte items do not fit a reply buffer
  worth keeping on every client; previews list, `read(seq)` fetches the one wanted.

## Open questions

- The background-write policy (Phase 3): a policyd capability, or focus-only writes for apps.
- Sync identity: the origin `device` field is in place; the account binding belongs to the
  dsoftbus consumer's RFC.
- A live "history changed" push to surfaces that did not copy (another app's copy while the
  keyboard's cards are open): it needs a notification channel of its own, since app-host's
  event inbox must keep windowd as its only sender (RFC-0079). Until then surfaces re-list on
  open, category change and filter, and the copying app refreshes itself (`ClipboardChanged`).

## Implementation Checklist

- [x] `nexus_wire::clipboardd` + tests · [x] `clipboardd` (history, gate, answer, os_lite)
- [x] topology, routes, slots, policy, volume list, init wiring, supervision
- [x] windowd focus push · [x] `svc.clipboard.*` + `svc.ime.insert` · [x] imed `OP_INSERT`
- [x] shell search · [x] keyboard toolbar + cards · [x] selftest probes · [x] lane + chain group
- [x] text-field copy / cut / paste + `ClipboardChanged` (board round 2026-10-07) · [x] lane copy step
- [ ] board rung `board-visual: clipboard` confirmed
