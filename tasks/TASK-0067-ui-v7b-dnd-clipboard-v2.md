---
title: TASK-0067 UI v7b: clipboardd (the one clipboard authority — text history, focus-gated reads) + `svc.clipboard` binding + windowd focus truth
status: In Progress (2026-10-07 — recut with the operator: text only, drag and drop moved to TASK-0086, flavors stay TASK-0087's; RFC-0094 + ADR-0070; implemented and QEMU-proven; board cycle 1 done, its findings fixed under TASK-0067B — board cycle 2 pending)
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - UI v2a input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v6b app lifecycle baseline (focus): tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Clipboard History DSL follow-up: tasks/TASK-0067B-ui-v7b-clipboard-history-dsl-overlay.md
  - Policy as Code (clipboard guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (clipboard budgets): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
  - Contract: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md · Decision: docs/adr/0070-clipboardd-one-authority-focus-gated-reads-ui-in-shell-and-keyboard.md
  - Drag and drop (moved here 2026-10-07): tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
---

## Recut 2026-10-07 — text clipboard, no drag and drop (binding; supersedes the 2026-09-09 rewrite below where they differ)

Operator decisions (2026-10-07): 0067 and 0067B ship together; **drag and drop moves to
TASK-0086** (the Files app — the first real source of a drag, `text/uri-list` + grant tokens;
its windowd routing ops take numbers there, not 29–31: op 29 is TASK-0066's tile preview);
**text only** for now (flavors and the VMO path stay TASK-0087's); **sync over dsoftbus
with same-account devices is a non-goal but shapes the item** (every entry carries `seq`,
writer sid and origin device; sync becomes a consumer of the same history).

### Goal (end system)

ONE clipboard authority `clipboardd`: a bounded newest-first text history in fixed storage;
writes held by the route; reads gated by the kernel sender id against the owners windowd
pushes (focused window = paste; shell and keyboard = paste + history); a `svc.clipboard.*`
DSL binding; nothing in the shell, the keyboard or windowd keeps its own copy.

### Delivered 2026-10-07

- **Wire** `nexus_wire::clipboardd` (`'C','B'` v1): `OP_WRITE/READ/LIST(query)/RESTORE/CLEAR`,
  `OP_FOCUS` (windowd → clipboardd, never answered); bounds text 248 B, preview 120 B, query
  64 B, history 16; round-trip + `test_reject_*` tests.
- **clipboardd** (`source/services/clipboardd`, replaces the placeholder): `history.rs` (fixed
  storage, dedupe-to-top, eviction, restore re-stamps, `seq` never reused), `gate.rs` (Write =
  route holder; Read = focused/shell/keyboard; History = shell/keyboard; Focus = windowd's sid;
  sid 0 = nobody), `answer.rs` (request → reply + an `Outcome` the markers come from — never
  contents), `os_lite.rs` (declared server, `serve_next`, no reply without a cap, bounded
  noisy lines, circuit breaker); `tests/contract.rs` (10 tests). `userspace/clipboard` deleted;
  ADR-0008 superseded.
- **Topology** `ServiceId::Clipboardd = 33` (COUNT 34), routes windowd/execd/harness →
  clipboardd, slots (windowd 15, execd 27, harness 0x3B), spec (Platform stage, pure server),
  policy `clipboardd = ["ipc.core"]`, `config/os-services.txt`, the system volume (cap raised
  16 → 24 with a test instead of a silent `take`), supervision `Standard/OnFailure`, init's
  pre-minted pair (the four identical pre-mint blocks folded into one helper — the
  orchestrator shrank 734 → 702 LOC), windowd's leg via its declared spec, execd's named route.
- **SDK route** `svc.clipboard` → child slot 23, permission `nexus.permission.CLIPBOARD`
  (abilitymgr knows it). Found on the way: `svc.updates` sat on child slot 19 — the glyph
  atlas's — and failed every boot (`execd: FAIL app route grant svc=updates`); moved to 22,
  and `test_reject_child_slot_collision` now checks every fixed app-child slot.
- **windowd** `runtime/clipboard_focus.rs`: the (focused, desktop, IME) owners recomputed on the
  frame pass, deduped, NONBLOCK with an owed retry. `shell_band.rs` (pure, tested): presses,
  hover and wheel inside the shell's panel glass — which the scene already composites above
  app windows — and anywhere while the shell holds a modal go to the shell; before, the
  Control Center over a window was visible but its presses reached the window.
- **app-host** `effect_clipboard.rs` (`list/read/write/restore/clear`, denials →
  `ERR_SVC_DENIED`, marker `apphost: dsl svc clipboard.restore ok (seq=…)`).
- **Proof**: `SELFTEST: clipboard gate ok` (every lane: six stored items — the operator's
  prefill — plus READ/LIST refused to the harness and a lying length MALFORMED);
  `usb-visible`: `clipboardd: focus truth live`, `apphost: dsl svc clipboard.restore ok`,
  `SELFTEST: ui v7 clipboard ok` (chain group `clipboard`); board rung `board-visual: clipboard`.

- **Found and fixed by the first full gate (blocking, same code path — init's service set)**:
  the 33rd service pushed init's respawn TIMER out of its responder waitset — the kernel
  capped a waitset at 32 members and init added every control channel first, the timer last,
  ignoring the error. On smp1 pinched's deliberate crash was never followed by a restart and
  init slept (the run ended on the grace timer, `init: supervision persist restarts=0x`
  missing). Fix at the root: the kernel bound is 64 (`MAX_WAITSET_MEMBERS`, the wait path's
  stack snapshot 256 B), mirrored as `nexus_abi::WAITSET_MEMBERS_MAX` and held equal by
  `check-ipc-bounds.sh` (rule 1b); init adds its timer FIRST and names every refused member
  (`init: FAIL responder waitset add …`); nexus-init's host test
  `the_responder_waitset_holds_every_service_and_the_timer` checks the topology against the
  bound.

- **Board cycle 1 (2026-10-07)**: copy and paste inside text fields were missing — built in
  the TASK-0067B board round (keyboard editing core, Ctrl+A/C/X/V over the app's own route,
  the `ClipboardChanged` trigger; RFC-0094 "Copy, cut and paste in text fields"). The
  service side changed in one place: `clipboardd: write ok` names the seq only — a length per
  copy would trace what the user typed (`markers_name_numbers_never_contents`).

### Open findings (recorded, not built)
- Background writes by permission holders are allowed (the mobile reference's model);
  a policy for them is TASK-0087's together with flavors.

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09 (verified in code):** `source/services/clipboardd/src/main.rs` is a
14-line placeholder (`STATUS: Placeholder`, prints `clipboardd: ready`, calls `clipboard::run()`
— an unregistered marker with no behaviour behind it); `userspace/clipboard/src/lib.rs` is a
host-only `Mutex<Option<String>>`; no `nexus_wire` clipboard module, no RFC, no init/topology
route, nothing consumes it. DnD: nothing beyond vendored cursor shapes (`userspace/ui/cursor/
src/hotspot.rs`). Successors that presume a clipboardd: TASK-0087 (html/rtf/image flavors) and
TASK-0122C (DSL bridge) — both absorbed here so the service is multi-MIME from day one and
usable from `.nx` on day one (no second clipboard ledger owns a second shape).

### Goal (end system)

One clipboard authority (`clipboardd`): multi-MIME items, bounded history, reads gated by
window focus (identity from `sender_service_id`, focus truth pushed by windowd), a
`svc.clipboard.*` DSL binding; DnD = windowd hit-test/routing + focus semantics ONLY, payload
bytes move through clipboardd's one-shot transfer items; drag image = a source-owned surface
windowd positions and never draws.

### Non-goals

Persistence across boots (clipboard is ephemeral, nothing in statefs); cross-device sync;
format conversion (a consumer picks a flavor, nobody transcodes); clipboard/DnD UI (0067B);
windowd drawing anything.

### Invariants

- Identity = `sender_service_id`; a read is allowed only for the focused window's owner sid or
  the desktop owner sid (history surface); `DenyReason::ClipboardFocus` ("clipboard-focus") is
  audited.
- Bounds: `FLAVORS_MAX = 8`, `MIME_MAX = 64 B`, `INLINE_MAX = 384 B` (fits os-lite
  `MAX_FRAME = 512`), `ITEM_BYTES_MAX = 1 MiB` (VMO path), `HISTORY_MAX = 16` ring;
  deterministic MIME preference = request order; stable deny/status codes.
- No `unwrap`/`expect` on wire input; `test_reject_*` for every bound.

### Decisions

- **D1 RFC-0094 "Content transfer v1"** seeds both wires. `nexus_wire::clipboardd` (`'C','B'`,
  v1): `OP_WRITE=1` (inline flavors), `OP_WRITE_VMO=2` (CAP_MOVE VMO, header-last with the ONE
  payload-VMO header of TASK-0033 D2), `OP_READ=3` (mime prefs → inline or `STATUS_TOO_BIG`),
  `OP_READ_VMO=4`, `OP_LIST=5` (seq, origin sid, flavor mimes + sizes), `OP_RESTORE=6`,
  `OP_CLEAR=7`, `OP_WATCH=8` / `OP_EVENT=9` (RFC-0078 push with resync bit), `OP_FOCUS=10`
  (windowd → clipboardd `{focused_sid, desktop_sid}`, retained latest-wins), `OP_TRANSFER_TAKE=11`
  (DnD one-shot read by seq + token). Status: `OK, MALFORMED, DENIED, TOO_BIG, EMPTY,
  UNKNOWN_SEQ, NO_FLAVOR`.
- **D2 Placeholders DELETED, settingsd shape adopted.** `userspace/clipboard` and the 14-line
  main go; clipboardd = `src/{lib,main,os_lite,ring,gate,transfer}.rs` + `tests/`,
  `[package.metadata.nexus-service]` (feature SSOT), volume bundle
  (`scripts/system-volume-services.txt`), `config/os-services.txt`; no `service-layout.allow`
  entry (tests/ mandatory). ADR "clipboardd is the single content-transfer authority" (docs/adr,
  next free). Gate: `userspace/clipboard` removed from the root workspace; a structure test
  fails if a second clipboard store appears.
- **D3 Focus truth is pushed, never asked.** windowd sends `OP_FOCUS` on every focus change
  (the `windows_feed.rs` NONBLOCK + owed discipline); clipboardd never calls windowd.
- **D4 DSL surface.** `tools/nexus-idl/schemas/dsl_services.capnp` (sorted insert):
  `clipboard.write(Str mime, Str text) → Bool`, `clipboard.read(Str mime) → Str`,
  `clipboard.list() → List<ClipEntry>`, `clipboard.restore(Int) → Bool`, `clipboard.clear() →
  Bool`; app-host `effect_clipboard.rs`; `nexus-sdk-routes` row `{svc: "clipboard", route:
  "clipboardd", permission: "nexus.permission.CLIPBOARD", child_slot: 20}`;
  `abilitymgr/src/caps.rs::KNOWN_PERMISSIONS` += `nexus.permission.CLIPBOARD`. Host mock =
  `TranscriptHost`.
- **D5 DnD in windowd = routing only.** `OP_SURFACE_DND_START=29` (source: mimes ≤ 8),
  `OP_SURFACE_DND_TARGET=30` (windowd → app: ENTER/OVER/LEAVE/DROP, surface-local xy),
  `OP_SURFACE_DND_ACCEPT=31` (target: mime index | reject reason); drag image = a source-owned
  `Overlay`-role surface with intent flag `INTENT_FLAG_FOLLOW_POINTER` (windowd positions it,
  never draws it); pointer grab to the source; hit-test over `hit_order`; Escape cancels; no
  input to non-targets. Bytes: source writes a transfer item (`OP_WRITE` flag `TRANSFER` →
  seq + token), the accepted target takes it exactly once (`OP_TRANSFER_TAKE`).
- **D6 Topology only via the TASK-0324 P4 declarative arm** — never a bespoke init arm:
  `ServiceId::Clipboardd = 31`, `ServiceSpec{exposes_server, reply_inbox, routes_to: [Policyd]}`,
  `REQUIRED_ROUTES += (Execd→Clipboardd), (Windowd→Clipboardd), (Clipboardd→Policyd)`;
  `policies/base.toml`: `clipboardd = ["ipc.core", "policy.delegate"]`; background affinity.

### Packages

- **P0** RFC-0094 + ADR + capnp surface (approval zones); **`ServiceId::Clipboardd` = the next free id at P0 ("31" in D6 predates xhcid), COUNT bump, `check-slot-ssot`**. Blast: paper.
- **P1** Wire module + clipboardd core (ring/gate/transfer) + host tests. Blast: `just check`,
  nexus-wire tests.
- **P2** os-lite entry + topology/policy/volume entries + markers. Blast: smp1 + visible lanes,
  volume boot (`bundle served` count), policy lanes.
- **P3** windowd `OP_FOCUS` push + DnD routing (`compositor/runtime/dnd.rs`,
  `windowd/tests/dnd_routing.rs`). Blast: input lanes (`input-live`), RFC-0086 consumers.
- **P4** app-host binding + docs.

### Definition of Done

Host: multi-MIME write/read, deterministic eviction order, focus deny, transfer one-shot,
`test_reject_oversized_item`, `test_reject_unknown_seq`, `test_reject_foreign_sid`,
`test_reject_too_many_flavors`; DnD negotiation fixtures (offer `{text/plain, image/png}` →
target selects → drop; reject reason deterministic). QEMU (registered in `proof-manifest/
markers/ui.toml` + `end.toml`, `scripts/qemu-test.sh`, `markers.txt` via the windowd contract):
`clipboardd: ready` (only after the ring + focus gate are live), `clipboardd: write ok
(flavors=2 bytes=…)`, `clipboardd: read ok (mime=text/plain)`, `clipboardd: read deny
(reason=clipboard-focus)`, `windowd: dnd start (mimes=2)`, `windowd: dnd drop ok
(mime=text/plain)`, `SELFTEST: ui v7 clipboard ok`, `SELFTEST: ui v7 dnd ok`. Docs:
`docs/dev/ui/patterns/transfer-sharing/{clipboard,drag-and-drop}.md` rewritten,
`docs/dev/dsl/services.md`.

### Touched paths

`source/services/clipboardd/**`, `source/libs/nexus-wire/src/clipboardd.rs`,
`source/libs/nexus-display-proto/src/surface_dnd.rs`, `source/services/windowd/src/compositor/
runtime/{dnd.rs,input.rs}`, `source/services/app-host/src/effect_clipboard.rs`,
`source/libs/nexus-sdk-routes/src/lib.rs`, `source/init/nexus-init/src/service_topology.rs`,
`policies/base.toml`, `config/os-services.txt`, `scripts/system-volume-services.txt`,
`tools/nexus-idl/schemas/dsl_services.capnp`, `userspace/clipboard` (deleted).

### Dependencies

TASK-0324 P3/P4 (routing v2 + declarative arm); TASK-0054C (`KernelClient::call` is the
request/reply API this service is written against); TASK-0033 D2 (payload-VMO header);
TASK-0066 D3 (feed flag bits precede any drag flags).

## Rebase (2026-08-14) — historical, superseded by the end-state rewrite above

### Verified reality — the baseline is near zero

- Clipboard today is a **host-only** `Mutex<Option<String>>`
  (`userspace/clipboard/src/lib.rs:31`);
  `source/services/clipboardd/src/main.rs` is a **14-line placeholder**; and
  there is **no clipboard module in `source/libs/nexus-wire/src/`** — no OS
  wire protocol exists at all.
- DnD: nothing exists beyond vendored cursor shapes
  (`userspace/ui/cursor/src/hotspot.rs:31-34` — `dnd-none/copy/move/link`).

### Ownership — this task builds the real clipboardd

Successors TASK-0087 (clipboard v3 upgrade) and TASK-0122C (DSL bridge) both
presume a working clipboardd; no other ledger owns it. **This task does.**
Model the service on settingsd (TASK-0072, Done): a real no_std OS entry
point (os-lite), a new `nexus_wire::clipboardd` wire module, and settingsd's
registry/persist pattern — no parallel service shape.

### Boundary fix (docs/dev/ui/windowd-cleanup-map.md:4-9)

windowd = Single Present Authority (compositor SERVICE). The draft's
"drag image overlay (VMO-backed)" inside windowd violates that boundary: the
**drag image is a client/app surface** (the dragging app or the DSL shell app
presents it). windowd gets ONLY:

- DnD **hit-test/routing**: enter/over/leave/drop targeting, and
- **focus semantics** during a drag (no background input leak).

No DnD rendering, no overlay drawing, no clipboard UI in windowd (the history
UI is TASK-0067B's DSL overlay). Never build into a MOVE/DELETE file.

### Process gate

**RFC seed required before implementation**: the clipboard wire protocol
(`nexus_wire::clipboardd`) and the DnD routing ops are new service API/wire
formats → `docs/rfcs/RFC-TEMPLATE.md`, next free number, RFC index update.
`source/libs/**` and `docs/rfcs/**` are approval zones.

### Corrected proof + touched paths

`tests/ui_v7b_host/` never existed, and windowd has no `idl/*.capnp` — ops
live in wire-module libs. Host proofs go to `source/services/clipboardd/tests/`
(service layout: src/ + tests/) and `source/services/windowd/tests/` for DnD
routing. Allowlist below updated.

## Context — historical, superseded by the end-state rewrite above

To make the system productive, we need interoperable content transfer:

- drag-and-drop with typed payload negotiation,
- a robust clipboard with MIME support and history.

Both must be policy-guarded (focus/foreground constraints) and bounded (budgets).

Screenshot/share is handled separately (v7c).

## Goal — historical, superseded by the end-state rewrite above

Deliver:

1. DnD protocol + routing in `windowd` (routing/hit-test/focus ONLY — see
   rebase boundary fix):
   - DragSource/DropTarget interfaces
   - global DnD controller: enter/over/leave/drop targeting
   - drag image = client/app surface (bounded VMO); windowd routes it, never
     draws it
   - negotiated pull (`read(mime)` after accept)
2. Clipboard v2 service `clipboardd`:
   - multi-MIME items
   - history ring with configurable size and eviction
   - policy gating: focused/foreground subjects
3. SystemUI integration hooks (minimal):
   - clipboard history popup stub (optional for v7b)
   - full visible DSL clipboard history UI is a follow-up in `TASK-0067B`
4. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Full OS-wide file manager integration.
- Screenshot/share sheet (v7c).

## Constraints / invariants (hard requirements)

- Deterministic negotiation:
  - stable MIME preference order,
  - stable accept/reject reasons.
- Bounded memory:
  - cap drag image bytes,
  - cap clipboard item bytes,
  - cap history length.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`source/services/clipboardd/tests/` + `source/services/windowd/tests/`
(corrected 2026-08-14; `tests/ui_v7b_host/` never existed):

- DnD negotiation:
  - offer `{text/plain,image/png}` → target selects `text/plain` → drop ok
  - reject case produces deterministic trace/reason
- clipboard:
  - write multi-MIME item
  - read preferred mime returns expected data
  - history ring evicts oldest deterministically

### Proof (OS/QEMU) — gated

UART markers:

- `windowd: dnd on`
- `dnd: enter(target=..., mimes=...)`
- `dnd: drop ok (mime=...)`
- `clipboardd: ready`
- `clipboard: write ok (mimes=...)`
- `clipboard: read ok (mime=...)`
- `SELFTEST: ui v7 dnd ok`
- `SELFTEST: ui v7 clipboard ok`

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/clipboardd/` (replace the placeholder: src/ + tests/, os-lite, settingsd-pattern)
- `source/libs/nexus-wire/src/clipboardd.rs` (new wire module — approval zone `source/libs/**`)
- `source/services/windowd/` (DnD hit-test/routing + focus semantics ONLY; check the cleanup map first)
- `userspace/clipboard/` (host lib aligns to the same protocol shapes for host tests)
- `docs/rfcs/` (clipboard wire + DnD routing RFC seed — approval zone)
- `source/apps/selftest-client/`
- `docs/dev/ui/patterns/transfer-sharing/drag-and-drop.md` + `docs/dev/ui/patterns/transfer-sharing/clipboard.md`

## Plan (small PRs)

1. dnd IDL + controller + drag image overlay + markers
2. clipboardd + history ring + policy/budgets + markers
3. host tests + OS markers + docs + postflight
