---
title: TASK-0198 Supply-Chain hardening v2b (OS): Phase 1 device trust anchor + verifier verdict authority (lane-critical); Phase 2+ sigchain/translog/provenance enforcement + rotation
status: Draft
owner: @security
created: 2025-12-27
updated: 2026-08-25
depends-on: []
follow-up-tasks:
  - TASK-0289
links:
  - Contract (Phase 1): docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§4 trust model)
  - Supply-chain v2 host core (Phases 2+): tasks/TASK-0197-supply-chain-v2a-host-sigchain-translog-sbom-provenance.md
  - Supply-chain v1 baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Anchor pattern precedent: userspace/nxra/build.rs (BAKED_TRUST, RFC-0088)
  - Consumers: tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md, tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Testing contract: scripts/qemu-test.sh
---

## PHASE CUT 2026-08-25 (RFC-0089 lane recut)

This ledger is split into an immediately-executable Phase 1 (the OTA lane's
package 1 — no dependencies) and the original v2b enforcement scope as Phases
2+ (after TASK-0197). Rebased out of the old body: `updated`'s OTA
envelope-verify and apply-time anti-downgrade now live in RFC-0089 §3/§10 and
execute in TASK-0179 (this task must not duplicate them); boot-anchored
rollback is TASK-0289; the old "A/B booted slot truth remains TASK-0037" note
is dead (0037 → 0289; the loader proves booted-slot truth).

## Context — Phase 1 (a live security hole)

`userspace/updates/src/system_set.rs` (~:171) verifies the system-set signature
against `index.publisher` — a key READ FROM THE ARCHIVE BEING VERIFIED. Any
self-signed archive passes. Additionally
`source/services/updated/src/os_lite.rs` (~:627-630) re-adjudicates a keystored
"signature invalid" verdict with a local in-process verify — a verifier's
"invalid" must be final. The device publisher allowlist exists only on the host
bundlemgrd path; the OS update path has NO trust anchor.

## Goal

**Phase 1 — device trust anchor + verdict authority (lands first, standalone):**

- `policies/update-trust.toml` (publisher keys for system sets/OS images) baked
  via `userspace/updates/build.rs` into `updates::BAKED_PUBLISHERS` — exact
  nxra `BAKED_TRUST` pattern (strict parser, malformed file fails the build;
  proof key only until provisioning; production images replace the set).
  Runtime trust partition rejected for v1 (mutable state needing its own root);
  rotation extends the baked set via `rotation-record` components (Phase 2+).
- `system_set.rs`: publisher membership check BEFORE any signature use
  (`index.publisher ∈ BAKED_PUBLISHERS`, or an injected allowlist in host
  tests); reject otherwise with the stable reason `untrusted publisher`. The
  same check carries verbatim into the `.nxs` v2 manifest verifier (TASK-0179).
- `updated` verdict authority: keystored `Ok(false)` ⇒ HARD reject; the local
  fallback verify runs ONLY on keystored transport unavailability, is loud
  (`updated: verify fallback (keystored unavailable)`), and binds to the same
  baked set.

**Phases 2+ — v2b enforcement (after TASK-0197, scope narrowed):**

- Sigchain envelope + translog inclusion + SBOM-hash enforcement on the store/
  bundle install paths (`storemgrd`/`bundlemgrd`); provenance recording
  (append-only, `/state`-gated); optional "Verified" surface.
- Key rotation: `rotation-record` components (RFC-0089 §4 seam) — each record
  signed by an already-trusted key, persisted append-only, monotonic; extends
  the Phase-1 baked anchor, never replaces the mechanism.
- Store-side version anti-downgrade (SemVer + build counter) for app installs —
  distinct from the OS-image rollback floor (TASK-0179/0289 own that).

## Non-Goals

- OS-image apply-time downgrade rejection and floor raising (TASK-0179),
  boot-time backstop (TASK-0289), kernel changes, online transparency log.

## Constraints / invariants (hard requirements)

- Fail closed everywhere; stable error strings (`untrusted publisher`,
  "sigchain missing", "translog inclusion missing", "sbom hash mismatch",
  "downgrade denied"); no partial installs (verify-before-commit).
- Never log key material; anchors bake at build time; a verifier's verdict is
  final.

## Stop conditions (Definition of Done)

### Phase 1 — Proof (Host), required

- `test_reject_untrusted_publisher` (self-signed archive with valid internal
  signature ⇒ reject), `test_accept_baked_publisher`,
  `test_reject_keystored_says_invalid` (no local re-adjudication),
  `test_fallback_only_on_unavailable`; existing system-set negative tests stay
  green; trust-bake build-failure test (malformed toml).

### Phase 1 — Proof (OS/QEMU), headless ladder additions

- `updated: stage rejected (untrusted publisher)` +
  `SELFTEST: updates trust reject ok` (selftest presents a self-signed fixture);
  existing OTA ladder and `SELFTEST: ipc routing updated ok` unchanged
  (regression signal). Marker SSOT updated together (qemu-test.sh +
  markers.txt + proof-manifest).

### Phases 2+ — Proof

- As per the v2a/v2b suites once TASK-0197 lands:
  `SELFTEST: supply store install ok`, `SELFTEST: supply store tamper deny ok`,
  rotation accept/replay-reject vectors.

## Touched paths (allowlist)

- Phase 1: `userspace/updates/` (+ build.rs), `policies/update-trust.toml`
  (new), `source/services/updated/`, `tests/updates_host/`,
  `source/apps/selftest-client/` + proof-manifest, `scripts/qemu-test.sh`
- Phases 2+: `source/services/storemgrd|bundlemgrd|translogd?`, fixtures,
  `docs/supplychain/`

## Plan (small PRs)

1. **P1a**: trust bake + `system_set.rs` membership check + host tests.
2. **P1b**: verdict-authority fix in `updated` + fallback discipline + host tests.
3. **P1c**: OS deny lane + markers + docs sweep (closes the lane package).
4. **P2+**: after TASK-0197 — envelope/translog/provenance enforcement +
   rotation records (own PR ladder, re-planned then).
