# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` — `In Review` (intentionally held for task-level finalization).
- **contract seed (RFC)**: `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` — `Done` (closure state synced).
- **mode**: final task-level review/report sync while keeping scope frozen.
- **tier**: `production-grade` BASELINE for the Updates / Packaging / Recovery group (per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`). Full closure of that tier remains `TASK-0197 + TASK-0198 + TASK-0289`.

## Drift check vs `current_state.md`

- [x] Manifest format is canonical capnp (`manifest.nxb`, ADR-0020 + TASK-0007 closure). Prior "manifest format drift" RED flag is RESOLVED in TASK-0029.
- [x] SBOM = CycloneDX JSON is on-policy (ADR-0021 §"Derived views and authoring" names it explicitly as the interop example). Not a deviation.
- [x] `keystored` capnp today exposes only `getAnchors / verify / deviceId` — the v1 ABI bump (`IsKeyAllowed*`) is real, scoped, and CAUTION-zone per `.cursorrules` (schema diff is part of task review).
- [x] `nexus-evidence` is the source of reproducible-tar + secret-scan primitives. v1 reuses them READ-ONLY, no API change.
- [x] `keystored` / `policyd` / `bundlemgrd` ownership boundaries match `RFC-0015` (policy authority). Single allowlist authority = `keystored`. No parallel allowlist anywhere else.
- [x] Marker discipline (`12-debug-discipline.mdc`): stable-label markers only; publisher / key / fingerprint values go to logd audit records, never into marker strings.

## Acceptance criteria (must be testable per cut)

### Host (mandatory)

- `test_reject_unknown_publisher` → stable error `policy.publisher_unknown`.
- `test_reject_unknown_key` → stable error `policy.key_unknown`.
- `test_reject_unsupported_alg` → stable error `policy.alg_unsupported`.
- `test_reject_payload_digest_mismatch` → stable error `integrity.payload_digest_mismatch`.
- `test_reject_sbom_secret_leak` → pack step refuses (reuse `nexus-evidence::scan` semantics).
- `test_reject_repro_schema_invalid` → `repro-verify` rejects.
- `test_reject_audit_unreachable` → `bundlemgrd` install fails closed when logd capability unwired.
- Two consecutive packs of the same inputs are byte-identical (SBOM + repro determinism floor).

### OS / QEMU (gated; only when OS install path is wired)

Stable-label markers, registered in `source/apps/selftest-client/proof-manifest/markers/`, attached to a profile under `proof-manifest/profiles/`:

- `bundlemgrd: sign policy allow ok`
- `bundlemgrd: sign policy deny ok`
- `SELFTEST: sign policy allow ok`
- `SELFTEST: sign policy deny ok`
- `SELFTEST: sign policy unknown publisher rejected ok`
- `SELFTEST: sign policy unknown key rejected ok`
- `SELFTEST: sign policy payload tamper rejected ok`

`verify-uart` deny-by-default enforces the ladder. Any QEMU evidence bundle produced for this task seals cleanly under both `--policy=bringup` and `--policy=ci`.

## Security checklist (mandatory per `.cursorrules`)

- [x] Threat model: publisher/key spoofing, allowlist bypass, payload tamper, SBOM tamper/injection, repro-meta forgery, secret leak via SBOM/repro, marker spoof, audit suppression.
- [x] Invariants: single allowlist authority in `keystored`, deny-by-default, identity from `sender_service_id`, bounded inputs, no secrets via `nexus-evidence::scan`, audit-or-fail via logd, deterministic markers, `SOURCE_DATE_EPOCH` only.
- [x] `DON'T DO`: parallel allowlist, "warn and continue", payload-string identity, raw key/signature/SBOM logging, skip audit emission, wall-clock timestamps, parallel manifest format under `meta/`, in-band publisher allowlist update without `keystored` rotation.
- [x] Reject-path proofs listed above are part of the task acceptance, not optional.
- [x] QEMU markers are gated, stable-label, manifest-registered.

## Production-grade tier — 10 hard gates (anti-drift guard for v2/v3)

These are mechanically enforceable in review/CI and exist so v1 stays *baseline* and does not freeze a contract that v2/v3 must change.

1. **No sigchain envelope.** No binding capnp envelope around `(manifest, sbom, repro, signature)`. Owned by `TASK-0197`.
2. **No transparency / Merkle translog.** No append-only install/update log, no Merkle root recording, no auditor-replay. Owned by `TASK-0197`.
3. **No SLSA-style provenance records.** v1 captures build env in `repro.env.json` only. Owned by `TASK-0197`.
4. **No anti-downgrade enforcement.** No rollback indices / monotonic version counters / downgrade-rejection. Owned by `TASK-0198 + TASK-0289`.
5. **No `updated` / `storemgrd` install-path changes.** v1 touches `bundlemgrd` only on the install path. Owned by `TASK-0198`.
6. **No boot-anchor / measured-boot work.** v1 stays inside the running OS. Owned by `TASK-0289`.
7. **Schema-extensibility ratchet** for the `keystored` capnp bump: explicit `@N` IDs + reserved gap (`@3..@7` request, `@2..@5` response); `reason: Text` (open set); `alg: Text` (open set).
8. **`meta/` layout-extensibility ratchet.** SBOM and repro under `meta/<file>` with content-hash-addressable references; v2 can append `meta/sigchain.nxb` without re-pack.
9. **Format policy compliance.** SBOM = CycloneDX JSON; no parallel TOML/YAML/protobuf carrier silently introduced. New bytes are either CycloneDX JSON or a small JSON schema-versioned descriptor.
10. **No marker drift.** Gating markers stable-label only; publisher/key/fingerprint values go to logd; markers registered in `proof-manifest/markers/`.

## Touched paths (allowlist, from TASK-0029)

- `tools/nxb-pack/` (embed SBOM + repro using `nexus-evidence` reproducible-tar primitives)
- `tools/sbom/` (new: CycloneDX 1.5 JSON generator)
- `tools/repro/` (new: repro metadata capture + `repro-verify` tool)
- `source/services/bundlemgrd/` (install-time enforcement; routes decisions through `policyd`)
- `source/services/keystored/` (allowlist check + key registry API impl)
- `source/services/policyd/` (allow/deny decision + audit context)
- `tools/nexus-idl/schemas/keystored.capnp` (**ABI** — CAUTION zone)
- `recipes/signing/` (new allowlist TOML)
- `source/libs/nexus-evidence/` (READ-ONLY)
- `tests/` (host tests, including `test_reject_*` set)
- `docs/supplychain/` (new docs)
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated marker update)
- `source/apps/selftest-client/proof-manifest/` (new markers + profile registration; do NOT touch `[meta]` schema)

## Suggested cut shape (8 cuts)

| Cut | Scope | Risk |
|---|---|---|
| C-01 | SBOM generator (`tools/sbom`) + reproducible embedding into `.nxb` under `meta/sbom.json`. | low |
| C-02 | Repro metadata + `repro-verify` (`tools/repro`). | low |
| C-03 | `keystored.capnp` ABI bump (`IsKeyAllowed*`) + parser + reject tests; schema diff explicitly part of review. | medium (ABI + CAUTION zone) |
| C-04 | `keystored` runtime impl (load `recipes/signing/publishers.toml`, deny-by-default). | low |
| C-05 | `policyd` decision + `bundlemgrd` enforcement order (verify → policy → digest → audit-or-fail). | medium (cross-service) |
| C-06 | Host `test_reject_*` set (7 cases). | low |
| C-07 | QEMU selftest probes + `proof-manifest/markers/` + profile registration (gated). | low |
| C-08 | Closure: docs (`docs/supplychain/{sbom,repro,sign-policy}.md`), `docs/testing/index.md`, RFC-0039 status flip + checklist tick, STATUS-BOARD / IMPLEMENTATION-ORDER sync. | trivial |

Reorder if cross-cut blockers emerge — keep C-03 → C-04 → C-05 ordering (ABI before runtime before enforcement).

## Closure delta queue (before `Done`)

1. Keep `TASK-0029` at `In Review` and only finalize task status when explicit task-level closure approval is given.

## Carry-over (TASK-0023B Phase-6 environmental closure)

`TASK-0023B` is `In Review`. Phase 6 is functionally closed; the single remaining environmental step is the external CI-runner replay of `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` per `docs/testing/replay-and-bisect.md` §7-§8. When the runner artifact lands:

1. Archive `.cursor/replay-ci.{json,log}` next to the four existing Phase-6 evidence JSONs.
2. Tick the Phase-6 box on `RFC-0038`.
3. Flip `TASK-0023B` `In Review` → `Done`.
4. Sync `STATUS-BOARD`, `IMPLEMENTATION-ORDER`, `.cursor/{handoff/current,current_state,next_task_prep}.md`.

This does **not** block kicking off TASK-0029.

## Linked contracts

- `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md`
- `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md`
- `docs/adr/0020-manifest-format-capnproto.md`
- `docs/adr/0021-structured-data-formats-json-vs-capnp.md`
- `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- `docs/rfcs/RFC-0016-device-identity-keys-v1.md`
- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `tools/nexus-idl/schemas/keystored.capnp`
- `source/libs/nexus-evidence/` (READ-ONLY)
- `source/apps/selftest-client/proof-manifest/`
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Done condition (current)

- Status flip to `Done/Complete` only after closure-delta queue is green and gate set is fully green:
  - `just dep-gate && just diag-os && just diag-host && just fmt-check && just lint && just arch-gate`
