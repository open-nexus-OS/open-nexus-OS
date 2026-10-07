# ADR-0070: The clipboard is ONE authority (`clipboardd`) whose reads windowd's focus truth gates; its UI lives in the shell search and the keyboard

- Status: Accepted
- Date: 2026-10-07
- Supersedes: ADR-0008 (Clipboard Architecture — the host-only `userspace/clipboard` library)
- Links:
  - Tasks: `tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md`, `tasks/TASK-0067B-ui-v7b-clipboard-history-dsl-overlay.md`
  - RFCs: `docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md` (the contract),
    `docs/rfcs/RFC-0075` (the `OP_INSERT` amendment), `docs/rfcs/RFC-0067` (windowd = compositor)
  - Related ADRs: `docs/adr/0068-*` (modal semantics — the search is a shell modal),
    `docs/adr/0069-*` (UI in the kit/shell, windowd draws nothing)

## Context

The clipboard was a placeholder service and a host-only mutex library (ADR-0008); nothing in
the OS could copy or paste, and no surface showed a history. The operator asked for the
clipboard in two places — the desktop shell's search (field + Apps / Files / Clipboard) and the
on-screen keyboard (the toolbar entry that swaps the keys for cards) — text first, with
cross-device sync kept in view. Three questions had to be decided once: where the data lives,
who decides who may read it, and where the UI lives.

## Decision

- **One authority.** `clipboardd` holds the history (16 text items, fixed storage, newest
  first, dedupe-to-top, monotonic `seq`, writer sid, origin device). Every surface is a client;
  no surface keeps its own copy. `userspace/clipboard` and the placeholder entry are deleted.
- **Reads are gated by identity the compositor knows.** Only windowd knows which window is
  focused, which app-host owns the desktop surface and which owns the IME overlay, so windowd
  PUSHES those three kernel sids (`OP_FOCUS`, retained latest-wins, computed on its frame
  pass); clipboardd compares each request's kernel `sender_service_id` with them. The focused
  window may paste; the shell and the keyboard may also browse and copy back; nobody else may
  read. Writing is held by the route (`nexus.permission.CLIPBOARD`). windowd decides nothing
  about the clipboard and draws nothing — it states facts.
- **UI where the user is.** The desktop shell owns the search (a modal layer on the desktop
  surface; its panels are panel-level glass, which the compositor already draws above app
  windows, and windowd now routes input there to match — `shell_band`); the keyboard app owns
  its toolbar and cards and inserts through imed (`OP_INSERT`, RFC-0075 amendment), never by
  writing into another app.
- **Room for what comes next, not built now.** Flavors and the VMO path (TASK-0087), drag and
  drop (TASK-0086 — routing in windowd, one-shot transfer items in clipboardd), sync (a dsoftbus
  consumer of the same history) extend the wire append-only.

## Consequences

- **Positive**: one truth for every surface; reads cannot be spoofed by payload bytes; the
  history is not browseable by ordinary apps; the shell's panels (Control Center, search) are
  usable above windows because input now follows the paint; the item shape already carries
  what sync needs.
- **Negative / accepted**: background writes by permission holders are allowed in v1 (the
  mobile reference's model) — the previous item stays in the history; Phase 3 adds a policy.
  The clipboard is ephemeral: a restart of clipboardd empties it.
- **Churn**: a new service (topology id 33, volume bundle, policy grant, supervision entry);
  windowd gains one push and one input rule; the DSL gains `.autofocus(true)` and grid cells
  that `.grow(1)` fill their track; the updates route moved off the atlas child slot it had
  collided with since TASK-0140.

## Alternatives considered

- **clipboardd asks windowd per read** — a call on every read, a cycle risk, and a race
  between the answer and the read. Rejected for the pushed, retained truth.
- **The history in the shell** — the keyboard needs it too; a second store is a second truth.
- **The search as its own overlay app** — windowd's overlay level is the docked keyboard band;
  a second overlay kind would be windowing UI in the compositor. The shell's modal on the
  desktop surface plus panel glass above windows reuses what exists.
