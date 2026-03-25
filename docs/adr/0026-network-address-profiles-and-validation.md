# ADR-0026: Network Address Profiles and Validation Semantics

Status: Accepted  
Date: 2026-03-24  
Owners: @runtime

## Context

Networking behavior spans multiple layers:

- runtime services (`netstackd`, `dsoftbusd`)
- harnesses (`scripts/qemu-test.sh`, `tools/os2vm.sh`)
- testing/docs contracts

Without a single authority for address profiles and validation rules, subnet/IP assumptions can drift across code and docs.
That increases the risk of non-standard, ad-hoc choices and future integration regressions.

## Decision

1) Keep one normative address-profile matrix in:

- `docs/architecture/network-address-matrix.md`

1) Treat profile values there as authoritative for:

- QEMU single-VM smoke (`qemu-smoke-*`)
- os2vm 2-VM deterministic profile (`os2vm-*`)

1) Require protocol-semantic DNS proof validation in runtime code:

- source port `53`
- DNS response bit (QR)
- probe TXID correlation

Do not require a fixed DNS source IP under QEMU/slirp because upstream response addresses may vary by host/network backend.

1) Any address-profile change must update, in one change set:

- this ADR (if policy changes),
- `docs/architecture/network-address-matrix.md`,
- affected runtime code,
- affected tests/harness checks.

## Rationale

- **Standards alignment**: prefer known QEMU usernet conventions where applicable (`10.0.2.0/24`).
- **Determinism**: keep os2vm profile deterministic and role-stable (`10.42.0.x` mapping).
- **No random drift**: prevent scattered literals from becoming de facto policy.
- **Debuggability**: semantic DNS validation is robust across backend variations.

## Consequences

- Networking/distributed docs link to one shared profile matrix.
- Harness and runtime contracts are easier to audit for drift.
- Introducing a new subnet/range now requires explicit governance.

## Related

- `docs/adr/0025-qemu-smoke-proof-gating.md`
- `docs/architecture/networking-authority.md`
- `docs/architecture/network-address-matrix.md`
