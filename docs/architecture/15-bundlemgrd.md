# `bundlemgrd` (bundle/package authority) — onboarding

`bundlemgrd` is the OS authority responsible for **bundle install/query/publication** and for serving payloads to the execution path (`execd`).

Related docs:

- Packaging contract (`.nxb` directory; `manifest.nxb` + `payload.elf`): `docs/packaging/nxb.md`
- Host-first manifest parsing (TOML schema): `docs/architecture/04-bundlemgr-manifest.md`
- Execution path context: `docs/architecture/10-execd-and-loader.md`
- Service architecture (direction): `docs/adr/0017-service-architecture.md`
- Policy/trust gating narrative: `docs/security/signing-and-policy.md`
- Testing + marker discipline: `docs/testing/index.md` and `scripts/qemu-test.sh`

**Scope note:** `docs/architecture/04-bundlemgr-manifest.md` is the host-first TOML parser; this page is the OS daemon authority.

## Responsibilities

- Verify and install bundles (policy-gated trust decisions live in the policy/security model).
- Publish installed bundles to the storage view used by `packagefsd`/`vfsd`.
- Serve payload bytes/manifests to other authorities (notably `execd`) via a stable RPC contract (as tasks define it).

## Updates v1 slot publication

`bundlemgrd` participates in the v1.0 update flow by supporting a soft switch:

- `OP_SET_ACTIVE_SLOT` re-publishes bundles from `/system/<slot>/`.
- The marker `bundlemgrd: slot <a|b> active` is emitted only after republication completes.
- The contract and markers are defined in `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`.

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
