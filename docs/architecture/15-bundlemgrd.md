# `bundlemgrd` (bundle/package authority) — onboarding

`bundlemgrd` is the OS authority responsible for **bundle install/query/publication** and for serving payloads to the execution path (`execd`).

Related docs:

- Packaging contract (`.nxb` directory; `manifest.nxb` + `payload.elf` + `meta/`): `docs/packaging/nxb.md`
- Host/OS manifest parsing contract (Cap'n Proto `BundleManifest`): `docs/architecture/04-bundlemgr-manifest.md`
- Execution path context: `docs/architecture/10-execd-and-loader.md`
- Service architecture (direction): `docs/adr/0017-service-architecture.md`
- Policy/trust gating narrative: `docs/security/signing-and-policy.md`
- Supply-chain v1 contract (SBOM/repro/sign-policy): `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md`
- Testing + marker discipline: `docs/testing/README.md` and `scripts/qemu-test.sh`

**Scope note:** `docs/architecture/04-bundlemgr-manifest.md` documents the canonical Cap'n Proto manifest contract; this page documents daemon authority/responsibilities.

## Responsibilities

- Verify and install bundles (policy-gated trust decisions live in the policy/security model).
- Publish installed bundles to the storage view used by `packagefsd`/`vfsd`.
- Serve payload bytes/manifests to other authorities (notably `execd`) via a stable RPC contract (as tasks define it).

## Updates v1 slot publication

`bundlemgrd` participates in the v1.0 update flow by supporting a soft switch:

- `OP_SET_ACTIVE_SLOT` re-publishes bundles from `/system/<slot>/`.
- The marker `bundlemgrd: slot <a|b> active` is emitted only after republication completes.
- The contract and markers are defined in `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`.

## System-volume verifier and reader (RFC-0089 §12, ADR-0060, TASK-0321)

bundlemgrd is the ONE authority that verifies the system volume (`system-a/b`)
and serves service ELFs from it: on the first volume op it attaches the
partition paired with the MEASURED boot slot over the block plane, verifies
the NXSV against the baked OS anchor (`policies/os-trust.toml`), pairs it with
the booted image digest + rollback line, reads the bounded pkgimg v3 index and
answers `QUERY_BUNDLE` / `GET_BUNDLE_ELF` (payload streamed into the caller's
VMO, hashed against the index digest, header written LAST) / `VOLUME_STATUS`.
Markers: `bundlemgrd: system volume verified (slot=<s> build=<id8> bundles=N)`,
`bundlemgrd: bundle served (name=…)`, `… FAIL (<reason>)`. Only the
kernel-attributed init id may pull ELFs; the read-only queries are open to the
boot-safe allowlist (the selftest cross-checks the flipped volume on the
`ota-bundle` lane).

## Non-goals

- Inventing parallel bundle formats (avoid `manifest.json` drift; `manifest.nxb` is canonical).
- Duplicating policy authority: decisions are owned by `policyd` and the security/trust model.

## Proof expectations

Bundle/package behavior must be proven via:

- host-first tests (deterministic, fast), and
- QEMU smoke markers in `scripts/qemu-test.sh` when OS bring-up needs end-to-end proof.

When you change manifest fields, trust gating, publication semantics, or exec handshakes:

- update the owning task stop conditions and proof commands,
- then update the architecture landings (`04-bundlemgr-manifest.md`, this page, and the contracts map/index).
