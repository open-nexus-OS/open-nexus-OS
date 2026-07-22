# RFC-0077: i18n v2 — compiled locale packs + runtime locale switch

- Status: In Progress (all phases proven 2026-07-21)
- Owners: @runtime
- Created: 2026-07-21
- Last Updated: 2026-07-21
- Links:
  - Tasks: `tasks/TASK-0240-i18n-l10n-v1_0a-host-catalog-compiler-icu-lite-plurals-deterministic.md`
    (host: packs + swap), `tasks/TASK-0241-i18n-v2-os-runtime-locale-switch-region-push-settings.md`
    (OS: region-driven switch + Settings)
  - Related RFCs: `docs/rfcs/RFC-0078-settings-region-keys-watch.md` (`ui.locale` key +
    watch spine), `docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md` (`OP_SURFACE_REGION`
    transport, already shipping tz/hour-format)

## Status at a Glance

- **Phase 0 (index-aligned packs + payload container, host)**: ✅ (TASK-0240 Done 2026-07-21)
- **Phase 1 (app-host catalog swap + reemit)**: ✅ (TASK-0241 Done 2026-07-21)
- **Phase 2 (windowd `ui.locale` watch + Settings language picker + de catalogs)**: ✅ (TASK-0241 Done 2026-07-21)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The locale-pack format (`NXL1`, index-aligned) and the payload container
    (`NXLC`) that ships packs with the app payload.
  - The runtime switch model: region push (`OP_SURFACE_REGION` locale field)
    → catalog swap → `view.reemit()`.
  - The fallback chain: active catalog → baked default text.
- **This RFC does NOT own**:
  - `OP_SURFACE_REGION` itself (landed with RFC-0076) or the `ui.locale`
    settings key (RFC-0078).
  - Plural rules, RTL, CLDR formatting (explicit follow-ups).
  - The DSL `@t()` semantics / NXIR layout (unchanged — TASK-0077 baseline).

### Relationship to tasks (single execution truth)

- TASK-0240 proves the pack/container/swap machinery on host; TASK-0241
  proves the OS loop (settings → windowd → app-host → re-render).

## Context

DSL i18n today: `@t("key")` compiles to `I18nExpr { key: index }`; the
DEFAULT catalog (`i18n/en.json`… actually the app's default file) is BAKED —
`I18nKey.key` points at the display-text symbol, so the dotted key NAMES are
NOT recoverable from NXIR at runtime. `LocaleSource` is already the runtime
seam (every emit resolves through it), and `Catalog`/`LocaleChain` exist
(TASK-0077/0078). What's missing: shipping non-default catalogs to the
running app and swapping them.

Because key names are absent at runtime, packs MUST be compiled at bundle
build time — where `Lowered.i18n_keys` provides the key order — into
**index-aligned** tables. No name matching at runtime, no l10nd, no Fluent,
no ICU4X (superseded lines TASK-0174/0175).

## Goals

- Language switching in Settings re-renders running apps live.
- Zero NXIR/compiler-semantics change; apps without extra catalogs unchanged.
- Fail-closed pack parsing; missing translations fall back to the baked text.

## Non-Goals

- Plural rules / gender; RTL; date-number CLDR formatting; per-app locale
  overrides; hot-reload from disk (packs travel with the payload).

## Constraints / invariants (hard requirements)

- **Determinism**: identical inputs ⇒ byte-identical packs (goldens).
- **No fake success**: `apphost: locale <tag> applied` only after the catalog
  actually swapped and the scene re-emitted.
- **Bounded resources**: pack entries ≤ 4096 keys, template ≤ 4 KiB each,
  container ≤ payload budget; parsing fail-closed (truncation/mutation ⇒
  `None` ⇒ baked default, never a panic).
- **Untrusted input**: payload containers ride the bundle path — every
  offset/length checked before use.

## Proposed design

### Pack format `NXL1` (normative)

Index-aligned to the program's `i18nKeys` table (same compile pass):

```
[4] magic "NXL1"
[4] entry count (u32 LE, == i18n key count)
per entry:
  [1] present flag (0 = fall back to baked text, 1 = template follows)
  [2] template byte length (u16 LE, present entries only)
  [n] UTF-8 template bytes (`{0}`-style placeholders, TASK-0077 semantics)
```

### Payload container `NXLC` (normative)

Replaces the raw `.nxir` payload for ui-program bundles (legacy raw NXIR
stays valid — consumers sniff the magic):

```
[4] magic "NXLC"   [1] version = 1   [3] reserved
[4] nxir length (u32 LE)   [4] zero padding
[nxir bytes]               (starts at offset 16 — capnp bytes stay 8-ALIGNED)
[1] pack count
per pack: [1] tag length, [tag bytes e.g. "de"], [4] pack length (u32 LE), [pack bytes]
[0-7] zero padding to a total length that is a multiple of 8
      (the bundle payload path requires 8-byte-multiple lengths)
```

### Runtime switch (normative)

- `Catalog::from_indexed_pack(bytes)` (runtime) parses `NXL1` fail-closed.
- app-host parses the container once at mount, keeps `(tag, Catalog)` pairs,
  and resolves `@t()` through `active catalog → baked default` (the baked
  default is the terminal — never the raw key for shipped apps).
- `OP_SURFACE_REGION`'s locale tag (e.g. `de-DE`) selects by primary
  language subtag (`de`); no matching pack ⇒ baked default (honest, no
  marker). On change: swap + `view.reemit()` + relayout +
  `apphost: locale <tag> applied` (bounded marker, tag only).
- windowd watches `ui.locale` as a SECOND subscription on its one
  init-provisioned push channel: each `OP_WATCH` cap-moves a SEND half, so
  windowd `cap_clone`s its half BEFORE the first move and subscribes
  `time.` + `ui.locale` separately (the RFC-0078 table keys by channel cap
  — two moved caps = two independent subscriber slots, no table change).

### Phases / milestones (contract-level)

- **Phase 0**: pack + container compilers (`nexus_dsl_core`), runtime parser,
  swap-reemit host goldens.
- **Phase 1**: app-host container parsing + locale plumbing (all
  `IdentityLocale` sites route through the app's active chain).
- **Phase 2**: windowd `ui.locale` watch; Settings language picker (de/en);
  German catalogs for settings; deterministic switch selftest.

## Security considerations

- **Threat model**: hostile pack/container bytes in a bundle (oversized
  lengths, truncations, non-UTF-8); marker spam via rapid locale flips.
- **Mitigations**: bounded fail-closed parsing (reject ⇒ baked default);
  templates render through the fixed `{n}` substitution only (no format
  interpretation); locale-applied marker bounded (≤8 per boot).
- **DON'T**: never render raw attacker bytes on parse failure — fall back to
  the baked default text.

## Failure model (normative)

- Malformed container ⇒ treat payload as raw NXIR if it parses, else the
  existing payload-failure path (no partial packs).
- Malformed pack ⇒ that locale unavailable (baked default), other packs
  unaffected.
- Unknown locale tag pushed ⇒ baked default, no marker, no error spam.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-dsl-core -p nexus-dsl-runtime -p dsl_goldens
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `SELFTEST: i18n switch ok` — selftest flips `ui.locale` en-US→de-DE;
  windowd relays; the greeter app-host applies both (markers
  `apphost: locale en-US applied` / `apphost: locale de-DE applied` observed)
- End state = shipped default (`de-DE`).

## Alternatives considered

- **l10nd service / Fluent+ICU4X** (old task families) — rejected:
  over-machinery; the `LocaleSource` seam + baked default already exist.
- **Name-keyed catalogs at runtime** — impossible without NXIR changes (key
  names are baked away); index-aligned packs avoid touching the IR.
- **Hot-reload catalogs from statefs** — rejected: packs are bundle
  artifacts; content updates ride bundle updates.

## Open questions

- Localized weekday/month names for the clock date line (currently English
  const names in app-host) — follow-up once app catalogs can carry them.

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

- [x] **Phase 0**: NXL1/NXLC compilers + runtime parser + swap goldens — proof: `cargo test -p nexus-dsl-core -p nexus-dsl-runtime -p dsl_goldens` (green 2026-07-21)
- [x] **Phase 1**: app-host container parsing + locale plumbing — proof: host tests + boot unchanged for pack-less apps (payload-invariant trap fixed: 16-byte header + 8-pad)
- [x] **Phase 2**: windowd ui.locale watch + Settings picker + de catalogs — proof: `SELFTEST: i18n switch ok` (ci-os-smp1) + `apphost: locale de-DE applied` (visible boot)
- [x] Task(s) linked with stop conditions + proof commands (TASK-0240/0241 Done).
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass (proof-manifest `routing.toml`/`bringup.toml`).
- [x] Security-relevant negative tests exist (`test_reject_malformed_packs_fail_closed`, `test_reject_malformed_containers`).
