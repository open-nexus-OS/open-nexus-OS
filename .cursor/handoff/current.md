# Current Handoff: TASK-0029 (Supply-Chain v1) — Kickoff Ready

**Date**: 2026-04-21
**Active task**: `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` — `Draft`
**Contract seed (RFC)**: `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` — `Draft`
**Tier**: `production-grade` BASELINE for the Updates / Packaging / Recovery group (per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`). Full closure of that tier is reached only at `TASK-0197 + TASK-0198 + TASK-0289`; v1 must stay on that trajectory without locking the wrong contract (10 explicit hard gates in TASK-0029 §"Production-grade tier").

## Status — what is ready, what is queued

### Ready (this session prepared)

- `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` audited end-to-end against current repo reality:
  - `depends-on` extended with `TASK-0007` (manifest.nxb baseline, `Done`).
  - `owner: @runtime @security`. ADR-0020, ADR-0021, TRACK-PRODUCTION-GATES, security standards, debug-discipline rule linked.
  - Context section rewritten — manifest.nxb already canonical (ADR-0020, TASK-0007 closure), `nxs-pack` + `updated/` already in tree, `nexus-evidence` reproducible-tar primitives reusable, `keystored` schema today exposes only `getAnchors / verify / deviceId` (no allowlist method — that is the v1 ABI bump).
  - Format-policy subsection added: SBOM is **CycloneDX JSON** under ADR-0021's interop bucket; canonical signed envelope is the **v2 sigchain** (`TASK-0197`), explicitly out of scope for v1.
  - Red flags: manifest format drift downgraded to **RESOLVED**. New **YELLOW** flags on `keystored` API surface (capnp ABI bump in CAUTION zone) and deterministic markers (no variable data in marker strings).
  - **Security section added** (mandatory per `.cursorrules`): threat model, 8 invariants (deny-by-default, single allowlist authority in `keystored`, identity from `sender_service_id`, bounded inputs, no secrets in artefacts via `nexus-evidence::scan`, audit-or-fail via logd, deterministic markers, `SOURCE_DATE_EPOCH` only), 8 `DON'T DO` items, 7 `test_reject_*` host proofs, 7 gated QEMU markers in stable-label form.
  - Plan §3 (Publisher/key allowlist policy) tightened: explicit capnp schema diff (`IsKeyAllowedRequest { publisher@0, alg@1, pubkey@2; @3..@7 reserved-for-v2 }`, `IsKeyAllowedResponse { allowed@0, reason@1 :Text; @2..@5 reserved-for-v2 }`, stable v1 reason labels `publisher_unknown / key_unknown / alg_unsupported / disabled`, open-set `Text` reason, open-set `Text` alg). `bundlemgrd` enforcement order: verify → policy (via `policyd` → `keystored::is_key_allowed`) → digest → audit-or-fail.
  - Production-grade tier section rebuilt: v1 baseline floor + 10 explicit hard gates (no sigchain envelope, no transparency translog, no SLSA provenance, no anti-downgrade, no `updated`/`storemgrd` install-path changes, no boot anchor, schema-extensibility ratchet, `meta/` layout-extensibility ratchet, format-policy compliance, no marker drift) + ASCII trajectory map.
  - Touched paths expanded with `policyd/`, `keystored.capnp` (ABI), `nexus-evidence/` (READ-ONLY), `selftest-client/proof-manifest/`.

- `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` authored from `RFC-TEMPLATE.md`:
  - Status `Draft`, owners `@runtime @security`, links to TASK-0029 + ADR-0020 + ADR-0021 + RFC-0012 / RFC-0015 / RFC-0016 / RFC-0011 / RFC-0038 + TRACK-PRODUCTION-GATES + future-RFC seeds (`TASK-0197 / 0198 / 0289`, named "do not own scope here").
  - 5-phase status-at-a-glance, scope boundaries (what RFC owns vs. does not own), context, goals, non-goals, constraints, proposed bundle layout (`<bundle>.nxb` + `meta/sbom.json` + `meta/repro.env.json`), CycloneDX 1.5 spec, repro schema, `keystored::IsKeyAllowed` capnp diff with reserved-field gaps, `bundlemgrd` enforcement order, security section condensed from TASK-0029, failure-model table with stable error labels, proof/validation strategy (host commands + QEMU markers), alternatives considered with rejection rationales, open questions, implementation checklist.

- `docs/rfcs/README.md` updated: `RFC-0039` added under "Security-relevant RFCs" and main "Index" with concise summary; bidirectional link from TASK-0029 → RFC-0039 in place.

### Queued (carry-over from prior session — do **not** re-litigate)

- **`TASK-0023B` is `In Review`** (status set 2026-04-20). Phase 6 is **functionally closed**. The single remaining environmental step is the external CI-runner replay of sealed bundle `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` per `docs/testing/replay-and-bisect.md` §7-§8. When the runner artifact lands:
  1. Archive `.cursor/replay-ci.{json,log}` next to existing `.cursor/replay-{dev-a,ci-like,synthetic-bad}.json` + `.cursor/bisect-good-drift-regress.json` (kept on disk as Phase-6 proof-floor evidence).
  2. Tick the Phase-6 box on `RFC-0038`.
  3. Flip `TASK-0023B` from `In Review` → `Done`.
  4. Sync `tasks/STATUS-BOARD.md` + `tasks/IMPLEMENTATION-ORDER.md` + `.cursor/{handoff/current,current_state,next_task_prep}.md`.

This step is **independent** of `TASK-0029` execution and does not block kicking it off.

## What the next chat session should do

1. Read this handoff + `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` + `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md`.
2. Confirm both can move from `Draft` → `Ready` (no open questions blocking execution; the 5 in RFC-0039 §"Open questions" are decision-level for v1, not blockers — pin the answers before code lands).
3. Author the Cursor-internal plan file in `~/.cursor/plans/` (single-cut cadence, host-first, OS-gated). Suggested cuts:
   - **C-01** SBOM generator (`tools/sbom`) + reproducible embedding into `.nxb` under `meta/sbom.json`.
   - **C-02** Repro metadata + `repro-verify` (`tools/repro`).
   - **C-03** `keystored.capnp` ABI bump (`IsKeyAllowed*`) + parser + reject tests; schema diff explicitly part of the review.
   - **C-04** `keystored` runtime impl (load `recipes/signing/publishers.toml`, deny-by-default).
   - **C-05** `policyd` decision + `bundlemgrd` enforcement order (verify → policy → digest → audit-or-fail).
   - **C-06** Host `test_reject_*` set (7 cases listed in TASK-0029 §Security).
   - **C-07** QEMU selftest probes + manifest registration (gated; no marker drift).
   - **C-08** Closure: docs (`docs/supplychain/{sbom,repro,sign-policy}.md`), `docs/testing/index.md`, RFC-0039 status flip + checklist tick, STATUS-BOARD / IMPLEMENTATION-ORDER sync.
4. Use the bundle `@task_0029_context` + `@task_0029_touched` (added in `.cursor/context_bundles.md`).
5. Keep the 10 hard gates from TASK-0029 §"Production-grade tier" mechanically in view at every cut — they are the anti-drift guard for v2/v3 tasks.

## Working-tree state at handoff

- `M .cursor/current_state.md`
- `M .cursor/handoff/current.md`
- `M .cursor/next_task_prep.md`
- `M docs/rfcs/RFC-0038-...md` (Phase-6 status sync from prior commit)
- `M tasks/TASK-0023B-...md` (status flip prep)
- `M uart.log` (test artifact only; **do not commit** outside a task closure — `.gitignore` documents the policy)
- New (uncommitted, prepared this session):
  - `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` (audited / extended)
  - `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` (new)
  - `docs/rfcs/README.md` (RFC-0039 index entry)
  - `.cursor/handoff/archive/TASK-0023B-...md` (snapshot copy of prior `current.md`)

## Phase-6 evidence kept on disk (do not delete)

- `.cursor/replay-dev-a.json` — native dev replay, `trace_diff.status == exact_match`.
- `.cursor/replay-ci-like.json` — containerized CI-like replay, `trace_diff.status == exact_match`.
- `.cursor/replay-synthetic-bad.json` — synthetic tamper, exit 1, `missing_marker[0].marker == "SYNTHETIC: tamper probe"`.
- `.cursor/bisect-good-drift-regress.json` — 3-commit smoke, `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`.

These four JSONs are the Phase-6 proof-floor evidence cited from `RFC-0038` and `tasks/TASK-0023B-...`. They stay until the external CI artifact lands and the closure mirror commit ships.
