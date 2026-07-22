# RFC-0078: Settings spine — region/keymap/time keys + OP_WATCH push propagation

- Status: In Progress (Phases 0–2 proven 2026-07-21)
- Owners: @runtime
- Created: 2026-07-21
- Last Updated: 2026-07-21
- Links:
  - Tasks: `tasks/TASK-0298-settings-spine-watch-region-keys.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`
    (keymap consumer), `docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md`
    (time.zone validator SSOT), `docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md`
    (ui.locale consumer)
  - ADRs: `docs/adr/0051-declarative-wire-codec-nexus-wire.md` (frames! codec)

## Status at a Glance

- **Phase 0 (keys + validators, host)**: ✅ (2026-07-21, `cargo test -p settingsd` 14 green)
- **Phase 1 (OP_WATCH/OP_EVENT wire + spine)**: ✅ (2026-07-21, codec goldens/reject + WatchTable host tests + QEMU `SELFTEST: settings watch ok`)
- **Phase 2 (inputd consumer + Settings pickers, QEMU)**: ✅ (2026-07-21, `inputd: keymap set us/de` live via push; Settings General pickers wired via `svc.settings`)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The region/input/time settings-key schema (names, defaults, validators, consumer map).
  - The settingsd change-propagation wire: `OP_WATCH`/`OP_EVENT` semantics and bounds.
  - The non-secret charter of this registry.
- **This RFC does NOT own**:
  - The theme/accent/shell keys (TASK-0072 baseline) or the prefs-blob format.
  - The windowd→app region push `OP_SURFACE_REGION` (RFC-0077).
  - Wall-clock/timezone conversion (RFC-0076) — only the `time.zone` key shape.
  - Per-key write ACLs (documented follow-up below).

### Relationship to tasks (single execution truth)

- TASK-0298 implements and proves every phase; consumer tasks (0147/0204/0241/0297)
  bind their keys through this spine.

## Context

settingsd has a typed, validated, persisted registry (5 keys) but **no change
propagation** — consumers re-read on their own cadence; theme only works
because windowd is its apply authority. The "General management" goals
(country, keyboard layout, timezone, hour format, IME personalization toggle)
need registry rows and a bounded push primitive so inputd/windowd apply
changes live.

## Goals

- One SSOT for the new keys with fail-closed validators.
- A bounded watch/push primitive every consumer can rely on (no polling).
- Live keymap switching as the first proven consumer.

## Non-Goals

- No l10nd/configd integration, no 2PC, no deep links, no per-app overrides.
- No secret material in this registry — ever (charter).

## Constraints / invariants (hard requirements)

- **Determinism**: validators are pure; watch events fire only on APPLIED
  changes (validate → persist → apply → notify).
- **No fake success**: `SELFTEST: settings watch ok` only after a watcher
  observes a real pushed change end-to-end.
- **Bounded resources**: ≤ 8 watch subscribers; prefix 1–64 B; per-subscriber
  queue depth 8 with drop-oldest + resync flag; fixed frame buffers, no
  per-event allocation.
- **Additive wire**: VERSION stays 1; existing OP_GET/OP_SET goldens
  byte-identical; unknown ops reject.
- **Security floor**: consumers trust `OP_EVENT` only on the connection they
  opened to settingsd; nothing sensitive enters the registry without a
  policyd gate (today: nothing sensitive at all).

## Proposed design

### Key schema (normative)

| key | default | validator | consumer (apply authority) |
|---|---|---|---|
| `ui.locale` (exists) | `de-DE` | BCP-47-ish (exists) | app-host locale packs via windowd relay (RFC-0077) |
| `region.country` | `DE` | exactly 2 ASCII uppercase | Settings display; formatting defaults follow-up |
| `input.keymap` | `de` | `us\|de\|jp\|kr\|zh` (= `keymaps::LayoutId` names) | inputd (`Keymap` swap, live) |
| `time.zone` | `Europe/Berlin` | membership in the tz-lite zone table (RFC-0076 SSOT; const mirror + pin test until tz-lite lands) | app-host clock via windowd relay |
| `time.format` | `24h` | `24h\|12h` | app-host clock |
| `ime.personalization` | `on` | `on\|off` | imed (TASK-0204) |

`input.keymap` defaults to `de` — the shipped QEMU profile is a German
keyboard; the first Settings win is switching it live.

### Wire contract (normative once Phase 1 proofs are green)

`nexus-wire/src/settingsd.rs`, additive (frames! codec, goldens + reject matrix):

- `OP_WATCH = 3` request: `prefix: str8(min=1, max=64)`. Reply = the standard
  response frame (`status`). Registers the cap-moved PUSH CHANNEL as a
  subscriber; a second watch moving the SAME channel cap replaces its prefix.
  One client may hold several subscriptions by moving DISTINCT caps
  (`cap_clone` a SEND half per prefix — windowd does this for
  `time.` + `ui.locale`, RFC-0077); each moved cap = one subscriber slot.
- `OP_EVENT = 4` push (settingsd → subscriber): `flags:u8, key:str8, value:str8`.
  `flags` bit0 = `resync`: events were dropped (queue overflow) — the
  subscriber must re-read its keys via OP_GET.
- Subscription lifetime = connection lifetime; a dead subscriber slot is
  reclaimed on send failure.

### Phases / milestones (contract-level)

- **Phase 0**: keys + validators (host tests).
- **Phase 1**: watch spine (host tests + codec goldens/reject matrix).
- **Phase 2**: inputd watches `input.` (live keymap swap, marker
  `inputd: keymap set <name>`), Settings General pane pickers
  (country + keyboard), QEMU selftest `SELFTEST: settings watch ok`.

## Security considerations

- **Threat model**: malformed watch requests; watch-queue exhaustion (DoS);
  event spoofing toward consumers; sensitive-value creep into the registry.
- **Mitigations**: codec-enforced bounds; subscriber cap + drop-oldest +
  resync; events only on the subscriber's own settingsd connection (kernel
  routing — no broadcast surface); non-secret charter pinned here.
- **Open risks / follow-up**: writes are UI-path-only by convention — a
  per-key write ACL (policyd-gated) is the recorded follow-up before any
  privileged key lands.

## Failure model (normative)

- Invalid prefix ⇒ `STATUS_MALFORMED`; subscriber table full ⇒
  `STATUS_PERSIST_FAIL`-class reject (no silent drop of the request).
- Queue overflow ⇒ drop-oldest + `resync` flag on the next delivered event —
  never a blocked settingsd loop.
- Consumer applies fail-closed: an invalid pushed value (impossible via
  validators, defense-in-depth) is ignored, never partially applied.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p settingsd -p nexus-wire
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `SELFTEST: settings watch ok` — set → watcher receives the pushed change
- `inputd: keymap set de` — keymap setting applied live (fires on change)

## Implementation notes (2026-07-21, discovered en route)

- **Subscription transport**: the OP_WATCH request CAP-MOVES the subscriber's
  push-channel SEND half (windowd `OP_SURFACE_EVENTS` pattern). inputd's
  channel is init-provisioned at fixed slots 0x20–0x22 (pre-minted in the
  orchestrator — init's cap table is at its ceiling by wiring time; the cap
  closes after wiring). The selftest mints its channel via `@mint-pair`
  (responder allowlist extended: execd + selftest-client).
- **UART interleaving**: concurrent subscriber prints garble marker lines and
  hard-fail evidence assembly — the probe settles 150ms (deadline-blocked
  recv, never a yield spin: spinning suppressed the kernel
  `KSELFTEST: runtime timer budget ok` proof).

## Alternatives considered

- **Polling consumers** (status quo) — rejected: N pollers × cadence beats
  one bounded push; live UI switches need immediacy.
- **windowd as universal relay without a watch primitive** — rejected:
  windowd would poll too; inputd (keymap) isn't windowd's concern.
- **configd 2PC** — rejected for these keys: presentation/typing prefs need
  no two-phase reload semantics.

## Open questions

- None blocking; per-key write ACLs tracked as the recorded follow-up.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: keys + validators — proof: `cargo test -p settingsd` (green 2026-07-21)
- [x] **Phase 1**: OP_WATCH/OP_EVENT + spine — proof: `cargo test -p settingsd -p nexus-wire` goldens + reject matrix + drop-oldest/resync (green 2026-07-21)
- [x] **Phase 2**: inputd consumer + Settings pickers — proof: `SELFTEST: settings watch ok` + `inputd: keymap set us`/`de` green in `ci-os-smp1` 2026-07-21
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`: oversized prefix, subscriber overflow, malformed events).
