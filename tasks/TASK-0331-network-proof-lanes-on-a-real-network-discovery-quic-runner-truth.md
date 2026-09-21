---
title: TASK-0331 Network proof lanes on a real network: cross-device discovery (W1), a peer for `quic-required` (W2), runner truth (W3h) — board ↔ QEMU/host
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @runtime
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Extracted from: tasks/TRACK-NETWORK-PROOF-LANES.md
  - Phase contract: docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md
  - Marker contract: source/apps/selftest-client/proof-manifest/markers/net.toml
  - 2-VM harness: tools/os2vm.sh, docs/rfcs/RFC-0010-*.md
  - Prerequisites: tasks/TASK-0248-*.md, tasks/TASK-0249-*.md (real NIC), tasks/TASK-0193-*.md (TLS)
---

## Origin

Extracted from `tasks/TRACK-NETWORK-PROOF-LANES.md` (W1/W2/W3h; `just ci-network` red since ≥ 2026-07-24). The track is dissolved; the network family `0024 → 0030 → 0038 → 0040` follows this ledger once its lanes are green.

## Context

dsoftbus cross-device discovery never happens in the 2-VM lane (`OS2VM_E_DISCOVERY_TIMEOUT`, 1 of 5 exit criteria met); `quic-required` demands a peer the single-VM profile cannot have; the declared `runner` is documentation only. With the board on a real network (N1/N2), the peer becomes real: board ↔ QEMU/host.

## Goal

`just ci-network` green with a real second device: discovery over the real link, a QUIC peer, the profile runner executed as declared; exit criteria of the dissolved track met and recorded here.

## Non-Goals

The network family's own content (0024 QUIC v2, 0030 discovery hardening, 0038 tracing, 0040 remote observability) — they run after this ledger.

## Packages (from the order file; the P0 rewrite fixes them)

- **N5** — W1 discovery on the real link, W2 peer for `quic-required`, W3h runner truth; then the family in order. Gate: `just ci-network` green + a two-device proof (board ↔ QEMU/host).

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
