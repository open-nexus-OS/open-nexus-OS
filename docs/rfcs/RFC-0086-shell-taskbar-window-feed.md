# RFC-0086: Shell taskbar window feed & taskbar verbs

- Status: In Progress (Phases 0–2 implemented + boot-proven 2026-07-30;
  marker-contract entries open)
- Owners: @ui @runtime
- Created: 2026-07-30
- Last Updated: 2026-07-30
- Links:
  - Tasks: `tasks/TASK-0313-launcher-design-handoff.md` (Track B, execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md` (retained latest-wins delivery doctrine)
  - Related docs: `docs/dev/ui/patterns/windowing/window-intent.md`, `docs/dev/ui/windowd-cleanup-map.md` (dock.rs = MOVE step 5)

## Status at a Glance

- **Phase 0 (identity floor)**: ✅ (execd `app:<id>` via exec_v2; windowd
  owner gates + `test_reject_*`)
- **Phase 1 (window feed + taskbar verbs)**: ✅ (ops 27/28; boot-proven
  `windows push` → `taskbar activate ok` → `restore`)
- **Phase 2 (legacy dock retirement)**: ✅ (`dock.rs` + compositor dock
  deleted; minimize/restore fly to the taskbar anchor)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The app-host **service-name identity scheme** (`app:<bundle_id>` via `exec_v2`) and who may assert it (execd only).
  - The **window feed** wire op (`OP_SURFACE_WINDOWS = 27`, windowd → desktop surface) — format, bounds, delivery semantics.
  - The **taskbar verb** wire op (`OP_SURFACE_TASKBAR = 28`, shell → windowd) — verbs, authority model, failure behavior.
  - The invariant that windowd stays **app-agnostic** (it never learns bundle-id strings; the sid↔app-id join happens in the shell's app-host).
- **This RFC does NOT own**:
  - abilitymgr recents/MRU payloads (`OP_RECENTS` stays count-only).
  - Per-tile minimize-animation geometry hints (parked follow-up; v1 animates to the taskbar band centre).
  - Window titles / per-window metadata beyond the running/minimized/focused flags.
  - Multi-window-per-app semantics (`MAX_APP_WINDOWS = 4` slots each carry one owner sid; several windows of one app id are simply several entries).

### Relationship to tasks (single execution truth)

- `tasks/TASK-0313-launcher-design-handoff.md` Track B (stages B0–B5) implements and proves every phase.

## Context

Minimized windows land in a compositor-drawn legacy dock (`windowd/src/dock.rs`,
self-labelled "MOVE → DSL-Shell-App, DO NOT EXTEND") that paints the same
hardcoded glyph for every window. The DSL shell's taskbar/dock shows installed
apps only — it has no idea what is running or minimized, and a tile click can
only launch. Three primitives are missing: window↔app identity, a
running-window feed to the shell, and an authorized "activate that window"
verb. The surface-control recv path additionally trusts a sender-supplied
surface-id byte (recorded follow-up in windowd `shell.rs` / app-host
`effect_ime.rs`: no per-sender identity, sid=0 observed) — the same missing
identity, in security clothing.

## Goals

- Kernel-attributed identity for app-host processes (`sender_service_id` ≠ 0).
- windowd pushes the running/minimized/focused window set to the desktop
  surface (the shell), keyed by owner sid — never by payload strings.
- The shell (and only the shell) can activate (restore/raise) an app's window.
- The legacy in-compositor dock is deleted; minimize targets the shell taskbar.

## Non-Goals

- A general cross-app window-management API (no app may act on another app's
  window; the desktop-surface owner is the single exception).
- Window enumeration for arbitrary clients.
- Recents/MRU semantics (abilitymgr's domain).

## Constraints / invariants (hard requirements)

- **Determinism**: feed pushes are change-driven with a dedupe against the
  last-sent encoding; markers fire only on real sends.
- **No fake success**: `windowd: taskbar activate ok` only after a real
  restore/raise took effect.
- **Bounded resources**: feed payload ≤ `4 + 1 + MAX_APP_WINDOWS × 9` bytes
  (41 today); NONBLOCK sends with an owed-retry flag (the
  `presentation_state` pattern) — a slow shell never wedges the compositor.
- **Security floor**: identity = `sender_service_id` from kernel IPC recv,
  never a payload byte. sid 0 is always refused. Deny-by-default: no
  matching slot / wrong sender → marker + no action.
- **Reachability**: a minimized window must never become unreachable — the
  legacy dock may only be deleted AFTER the taskbar activate path is proven
  (task ordering B4 after B3).

## Proposed design

### Identity: `exec_v2` service names (the keystone)

execd — the spawner, and the only honest asserter — spawns app-host instances
with `exec_v2(elf, prio, flags, "app:" + bundle_id)`. The kernel FNV-hashes
the name and stamps the task; `ipc_recv_v2` then surfaces it as
`sender_service_id` on every message. The `app:` prefix makes impersonating a
boot service (`imed`, `inputd`, …) impossible by construction — real services
carry no colon-prefixed names. Name length is bounded (≤ 64 bytes).

windowd's existing `owner_sid` capture at `SURFACE_CREATE`
(`app_window.rs`) becomes meaningful with zero wire change. windowd never
learns bundle-id strings: the shell's app-host computes
`service_id_from_name("app:" + entry.id)` per enumerate row (the userspace
FNV mirror in `nexus-abi`) and joins against the feed's sids.

The same sids close the recorded follow-up: the existing `CONTROL_WIN_*`
arms additionally require `apps[idx].owner_sid == sender_sid` (an app may
only minimize/close/mode/move its OWN window).

### Contract / interface (normative)

Envelope op space (append-only; retired 16/17 stay reserved): next free ops
27 and 28.

**`OP_SURFACE_WINDOWS = 27`** — windowd → the desktop surface's event
channel only.

```
u8  op = 27
u8  count                     (0 ..= MAX_APP_WINDOWS)
per window:
  u64le owner_sid             (kernel service id of the owning app-host)
  u8    flags                 bit0 = minimized, bit1 = focused
```

Presence in the list = the window is OPEN (running). Delivery: retained
latest-wins state (RFC-0083 doctrine) — windowd re-encodes on every
window-set mutation (open/close/minimize/restore/focus), dedupes against the
last-sent bytes, sends NONBLOCK; a failed send sets an owed flag retried on
the frame loop; the full set is re-sent on desktop (re)bind. Strict
length/count validation on decode; a malformed frame is dropped.

**`OP_SURFACE_TASKBAR = 28`** — shell → windowd, on the app-host's existing
windowd request channel.

```
u8  op = 28
u8  verb                      0 = ACTIVATE (restore if minimized, else raise+focus)
                              1 = MINIMIZE (reserved; not implemented in v1)
u64le target_sid
```

Authority gate (fail-closed, in order):
1. `sender_sid != 0` (else refuse — pre-identity senders have no authority).
2. `sender_sid == desktop_owner_sid` (captured at desktop `SURFACE_CREATE`;
   the greeter holding it pre-session is harmless — no app windows exist
   pre-session).
3. A slot with `owner_sid == target_sid` exists (else marker + no-op).

### Phases / milestones (contract-level)

- **Phase 0 — identity floor**: execd `app:` naming via `exec_v2`; windowd
  captures `desktop_owner_sid`; `CONTROL_WIN_*` owner checks. Proof:
  `test_reject_win_control_foreign_surface`, `test_reject_win_control_sid_zero`,
  minimize marker chain unchanged.
- **Phase 1 — feed + verbs**: op 27 encode/decode + push sites; shell
  app-host cache → `WindowsChanged` trigger → enumerate merge
  (`running`/`minimized`/`focused` fields); `svc.shell.activate` effect with
  launch fallback; op 28 gate. Proof: proto round-trips,
  `test_reject_taskbar_*` ×3, marker `windowd: taskbar activate ok`.
- **Phase 2 — legacy dock retirement**: delete `dock.rs` + dock parts of
  wm/input/transitions/scene/assets; minimize animation retargets to
  `(w/2, h − SHELL_TASKBAR_H/2)` (windowd owns the taskbar height as
  work-area policy, never tile positions). Proof: e2e marker chain
  launch → windows push → minimize → push → activate → restore.

## Security considerations

- **Threat model**: a malicious app-host forging taskbar verbs (blocked by
  the desktop-owner sid gate); an app acting on another app's window via the
  spoofable surface-id byte (closed by the owner check); service
  impersonation via chosen names (blocked by the `app:` prefix — execd is
  the only naming authority); replay (verbs are idempotent state moves);
  flood (NONBLOCK + existing queue bounds; feed is change-driven).
- **Mitigations**: kernel sender attribution end-to-end; deny-by-default
  resolution; bounded payloads.
- **Open risks**: the greeter (pre-session desktop owner) may send verbs —
  harmless today (no app windows pre-session), revisit if pre-session app
  windows ever exist.

## Failure model (normative)

- Unknown `target_sid` → `windowd: taskbar activate miss` marker, no action.
- Feed send failure → owed flag, retried next frame; never blocks compose.
- Shell dead ⇒ no desktop surface ⇒ verbs impossible — fail-closed by
  construction. A minimized window's reachability then depends on the shell
  respawning (stated openly; the session watchdog owns shell liveness).
- `svc.shell.activate` on a non-running app falls back to
  `svc.ability.launch` in the app-host effect layer (the DSL stays dumb).

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-display-proto surface_windows
cd /home/jenning/open-nexus-OS && cargo test -p windowd
cd /home/jenning/open-nexus-OS && cargo test -p app-host
cd /home/jenning/open-nexus-OS && cargo test -p execd
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 just test-os
```

### Deterministic markers (if applicable)

- `windowd: windows push (n=…)` — after a real feed send (change-driven).
- `windowd: taskbar activate ok` — after a real restore/raise.
- `windowd: taskbar activate miss` — unknown target (fail-closed path).
- `apphost: shell activate -> launch` — the not-running fallback.

## Alternatives considered

- **abilitymgr as the feed source** — abilitymgr knows lifecycle, not
  windows; minimized/focused is windowd's state SSOT (`window_scene`), and a
  windowd→abilitymgr mirror would fork it. Rejected.
- **Capability-minted taskbar channel** (windowd moves a SEND cap to the
  shell) — kernel-strong, but needs new cap-moving plumbing on the
  windowd→client event path for the same guarantee sid-gating already gives.
  Rejected for v1.
- **App-id strings in `SURFACE_CREATE` verified against the sid hash** —
  works, but redundant once the join lives shell-side; windowd would learn
  app ids for nothing. Rejected.

## Fallout of giving app-hosts identity (recorded)

Services that gated on "app-hosts have no identity" must be revisited when
this lands — the pre-identity code path was `sender_service_id == 0`:

- **bundlemgrd `OP_LIST_APPS`** accepted `sid == 0` (i.e. ANY unnamed task)
  for the public apps listing, with the follow-up recorded in-file. With
  execd naming, that condition stopped matching and the shell's app grid went
  empty (found on device, 2026-07-30). The gate now resolves the sender
  against the SAME registry it would list
  (`bundlemgrd::app_sender::is_registered_app_host`) — strictly tighter than
  before, and it cannot drift from the registry.
- Any future service adding an `sid == 0` allowance for app children should
  use the same registry-derived check instead.

## Open questions

- Per-tile minimize-animation targets (shell → windowd geometry hints) —
  parked; revisit with window drag/snap polish.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: identity floor — proof: `cargo test -p windowd control_gate` + `cargo test -p execd`
- [x] **Phase 1**: feed + verbs — proof: `cargo test -p nexus-display-proto` + `cargo test -p app-host` + reject tests; on-device chain 2026-07-30
- [x] **Phase 2**: legacy dock retirement — proof: minimize → taskbar tile → restore, boot-proven
- [x] Task(s) linked with stop conditions + proof commands (TASK-0313 Track B).
- [ ] QEMU markers appear in `tools/nx/chains/markers.txt` + `scripts/qemu-test.sh` (needs the chain-contract simulation — tracked in TASK-0313).
- [x] Security-relevant negative tests exist (`test_reject_*`: windowd control/taskbar gates, bundlemgrd app-sender).
