# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-22)**: `TASK-0029` closure-remediation pass completed for the critical drift set:
  - `manifest.capnp` now carries explicit `sbomDigest` / `reproDigest`; `nxb-pack` writes them and `bundlemgrd` verifies them.
  - Sign-policy path now routes through `policyd::supply_chain::decide_from_authority(...)` (no bundlemgrd-local label mapping).
  - `bundlemgrd` os-lite loop no longer ignores `sender_service_id`; unauthorized senders are denied.
  - Explicit size caps and reject coverage were added for SBOM/repro/install ingestion.
  - Full quality gate chain is green: `dep-gate && diag-os && diag-host && fmt-check && lint && arch-gate`.
  - RFC Phase-0 `cyclonedx-cli` roundtrip proof was captured successfully (`validate -> convert -> convert -> validate`, v1.5).
  - Status sync applied per user direction: `RFC-0039` set to `Done`; `TASK-0029` set to `Done`.
- **prev_decision (2026-04-21)**: `TASK-0029` (Supply-Chain v1) prepared for execution. Task audited end-to-end against current repo reality and architectural decisions:
  - `depends-on` extended with `TASK-0007` (manifest.nxb baseline, `Done`); `owner: @runtime @security`; ADR-0020 + ADR-0021 + TRACK-PRODUCTION-GATES + security/debug-discipline rules linked from header.
  - Context rewritten — `manifest.nxb` already canonical capnp (closing the prior "RED manifest format drift" flag); `nxs-pack` + `updated/` exist in tree but their SBOM/sigchain enforcement paths are owned by `TASK-0197 / 0198` (out of v1 scope).
  - Format policy locked per ADR-0021: SBOM = **CycloneDX JSON** (interop bucket, named example in ADR-0021); canonical signed envelope binding `(manifest.nxb, sbom.json, repro.env.json, signature)` is the **v2 sigchain** owned by `TASK-0197` — explicitly out of v1 scope.
  - **Security section** added (mandatory): threat model, 8 invariants (deny-by-default, single allowlist authority in `keystored`, identity from `sender_service_id`, bounded inputs, no secrets via `nexus-evidence::scan`, audit-or-fail via logd, deterministic markers, `SOURCE_DATE_EPOCH` only), 8 `DON'T DO` items, 7 `test_reject_*` host proofs, 7 gated QEMU markers in stable-label form.
  - Plan §3 specifies the explicit capnp ABI bump (`keystored::IsKeyAllowedRequest / IsKeyAllowedResponse`) with reserved-field gaps for v2 (schema-extensibility ratchet); `bundlemgrd` enforcement order: verify → policy (via `policyd` → `keystored::is_key_allowed`) → digest → audit-or-fail.
  - Production-grade tier section adds 10 explicit hard gates (no sigchain envelope, no transparency translog, no SLSA provenance, no anti-downgrade, no `updated`/`storemgrd` install-path changes, no boot anchor, schema-extensibility ratchet, `meta/` layout-extensibility ratchet, format-policy compliance, no marker drift) so v1 stays *baseline* without freezing a contract that v2/v3 must change. ASCII trajectory map: TASK-0029 → TASK-0197 → TASK-0198 → TASK-0289.
  - `RFC-0039` authored from template (Status `Draft`, owners `@runtime @security`), bidirectional link in `tasks/TASK-0029-...md`, indexed in `docs/rfcs/README.md` ("Security-relevant RFCs" + main index).
  - Prior `current.md` archived as `.cursor/handoff/archive/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`.
- **prev_decision (2026-04-20)**: `TASK-0023B` Phase 6 functionally closed. Replay/diff/bisect tooling delivered (`tools/{replay-evidence,diff-traces,bisect-evidence}.sh`, `scripts/regression-bisect.sh`, `docs/testing/replay-and-bisect.md`, `docs/testing/trace-diff-format.md` + fixtures). Hard gates verified locally: `--max-seconds`/`--max-commits` mandatory, `PROFILE` env override rejected by replay. Phase-6 proof-floor evidence stored in `.cursor/replay-{dev-a,ci-like,synthetic-bad}.json` + `.cursor/bisect-good-drift-regress.json` (kept on disk). Warm replay ~14s vs cold ~67s via persistent worktree/cache + `NEXUS_SKIP_BUILD=1`. RFC-0038 advanced `Draft` → `Done`; TASK-0023B advanced `Draft` → `In Review`.
- **prev_prev_decision (2026-04-17)**: TASK-0023B Phases 1-5 closed (structural extraction → maintainability → standards → marker manifest as SSOT → signed evidence bundles). New host-only crates `nexus-proof-manifest` + `nexus-evidence`. `selftest-client/main.rs` shrunk 122 → 49 LoC (dispatch-only). Marker ladder enforced via `proof-manifest/` (v2 split layout). Evidence pipeline wired into `scripts/qemu-test.sh` + `tools/os2vm.sh` with `CI=1` ⇒ seal mandatory.

## Active constraints (apply to TASK-0029 execution)

- **Single allowlist authority**: only `keystored` answers "is this publisher+alg+key allowed". `bundlemgrd` and `policyd` MUST NOT carry a parallel allowlist.
- **Deny-by-default**: unknown publisher / unknown key / unsupported alg / malformed signature / payload digest mismatch → reject install with stable error label; no "warn-and-continue".
- **Identity from channel**: `bundlemgrd` derives caller identity from `sender_service_id`, never from a payload string.
- **Audit-or-fail**: every allow/deny decision emits a logd audit event; if logd unreachable, install fails closed.
- **Deterministic markers** (`12-debug-discipline.mdc`): gating UART markers carry stable labels only (`bundlemgrd: sign policy allow ok` / `bundlemgrd: sign policy deny ok`); publisher / key / fingerprint values live in audit records, not in markers.
- **No secrets in artefacts**: SBOM / repro flow through `nexus-evidence::scan` deny-by-default before being packed; PEM / `*PRIVATE_KEY*=…` env-style / ≥64-char base64 high-entropy blobs refuse to seal.
- **Reproducible bytes**: SBOM and repro use `SOURCE_DATE_EPOCH`; reuse `nexus-evidence` reproducible-tar primitives (`mtime=0`, fixed gzip OS byte, lex-sorted entries) so two consecutive packs of the same inputs are byte-identical.
- **Bounded inputs**: explicit size caps on SBOM JSON, repro metadata, bundle entries; reject before parsing if exceeded.
- **Format policy** (ADR-0021): canonical signed bytes = capnp; interop = JSON. SBOM is JSON because that is the named ADR-0021 example, NOT a deviation. Any new bytes added to a bundle by v1 are either CycloneDX JSON or a small JSON schema-versioned descriptor.
- **`meta/` layout-extensibility ratchet**: v2 (`TASK-0197`) MUST be able to append `meta/sigchain.nxb` (capnp envelope) without re-pack and without changing the bundle wire format. v1 paths under `meta/` are stable.
- **Schema-extensibility ratchet** for the `keystored` capnp bump: named fields with explicit `@N` IDs + documented reserved gap (`@3..@7` request, `@2..@5` response) so `TASK-0197` can add transparency-log / sigchain fields without renumbering. `reason: Text` (open set), `alg: Text` (open set) so v2 can add classes / algorithms without breaking v1 callers.
- **Touched-paths discipline**: stay inside the TASK-0029 allowlist. v1 touches `bundlemgrd` only on the install path; `updated/` and `storemgrd/` are reserved for `TASK-0198`. `nexus-evidence/` is READ-ONLY in v1.
- Kernel untouched.
- No new forbidden crates in OS graph (`just dep-gate`).
- No blanket `allow(dead_code)`; no `unwrap`/`expect` on untrusted input.
- All 10 hard gates from TASK-0029 §"Production-grade tier" stay mechanically in view at every cut.

## Carry-over (Phase-6 environmental closure for TASK-0023B)

`TASK-0023B` is `In Review`. Phase 6 is functionally closed; the single remaining environmental step is the external CI-runner replay of the sealed bundle `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` per `docs/testing/replay-and-bisect.md` §7-§8. When the runner artifact lands:

1. Archive `.cursor/replay-ci.{json,log}` next to the four existing Phase-6 evidence JSONs.
2. Tick the Phase-6 box on `RFC-0038`.
3. Flip `TASK-0023B` `In Review` → `Done`.
4. Sync `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`, `.cursor/{handoff/current,current_state,next_task_prep}.md`.

This step is independent of TASK-0029 execution and does not block kicking it off.

## Active focus (execution)

- **active_task**: `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` — `Done` (final closure completed).
- **contract seed**: `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` — `Done` (contract/proof closure complete; task status finalized to `Done`).
- **active_plan**: `~/.cursor/plans/task0029_supply_chain_kickoff_bc589506.plan.md` executed; current step is final delta reporting.
- **tier**: `production-grade` BASELINE for the Updates / Packaging / Recovery group. Full closure is still only at `TASK-0197 + TASK-0198 + TASK-0289`; v1 must not pre-empt those scopes.

## Seed contracts (linked from TASK-0029 + RFC-0039)

- `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` (execution SSOT)
- `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` (contract seed)
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (tier policy)
- `docs/adr/0020-manifest-format-capnproto.md` (canonical = capnp)
- `docs/adr/0021-structured-data-formats-json-vs-capnp.md` (canonical-vs-interop split; SBOM JSON named here)
- `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (manifest.nxb baseline + bundle pipeline)
- `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy authority + audit contract)
- `docs/rfcs/RFC-0016-device-identity-keys-v1.md` (keystored / device keys baseline)
- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (audit sink contract)
- `docs/rfcs/RFC-0038-...-v1.md` (proof-manifest layout + evidence-bundle determinism reused here)
- `docs/security/signing-and-policy.md` (policy baseline)
- `docs/packaging/nxb.md` + `docs/packaging/system-set.md` (packaging direction)
- `tools/nexus-idl/schemas/keystored.capnp` (ABI to extend in v1)
- `source/libs/nexus-evidence/` (READ-ONLY in v1 — reuse `scan` + reproducible-tar primitives)
- Future RFC seeds (do not own scope here): `TASK-0197` (v2 host: sigchain + translog + SLSA), `TASK-0198` (v2 OS enforcement), `TASK-0289` (boot trust closure).

## Frozen baselines (must stay green per cut)

- Kernel: untouched across TASK-0029.
- `TASK-0023` / `TASK-0023B` Phases 1-5: closed/done baselines; do not regress.
- `TASK-0023B` Phase 6 tooling: replay/diff/bisect surfaces (`tools/{replay-evidence,diff-traces,bisect-evidence}.sh` + `scripts/regression-bisect.sh`) stay green.
- Host hygiene: `just dep-gate && just diag-os && just diag-host && just fmt-check && just lint && just arch-gate`.
- Evidence pipeline: `tools/verify-evidence.sh target/evidence/<latest>` returns 0 when seal is mandatory.
- Marker ladder: `pm_mirror_check` + `verify-uart` deny-by-default on every QEMU run; new TASK-0029 markers MUST be registered in `proof-manifest/markers/` and a profile under `proof-manifest/profiles/` before they can pass.

## Boundaries reaffirmed

- v1 stays scoped to **bundles** (`.nxb`); system-set (`.nxs`) SBOM + `updated`-stage enforcement are owned by `TASK-0197 / 0198`.
- v1 does **not** introduce: sigchain envelope, transparency / Merkle translog, SLSA-style provenance, anti-downgrade, install-path changes in `updated/` or `storemgrd/`, boot anchor / measured-boot.
- v1 does **not** carry a parallel allowlist outside `keystored`.
- v1 does **not** embed wall-clock timestamps anywhere — `SOURCE_DATE_EPOCH` only.
- v1 does **not** silently introduce a new structured-data carrier under `meta/` (only CycloneDX JSON or schema-versioned JSON descriptors).

## Linked tasks / contracts (broader context)

- `tasks/STATUS-BOARD.md` (project status)
- `tasks/IMPLEMENTATION-ORDER.md` (queue order)
- `tasks/TASK-0023B-...md` (carry-over: `In Review`, environmental closure pending)
- `docs/rfcs/RFC-0038-...md` (carry-over: `Done` modulo Phase-6 box)
