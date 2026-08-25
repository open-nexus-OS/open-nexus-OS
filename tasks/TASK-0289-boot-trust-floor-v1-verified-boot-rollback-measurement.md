---
title: TASK-0289 Boot trust floor v1: nxboot first-stage loader + boot flip (Phase A) and backstop proofs + measured surface (Phase B)
status: Draft
owner: @security @runtime @updates
created: 2026-04-13
updated: 2026-08-25
depends-on:
  - TASK-0315   # single GPT disk with bsb/boot-a/boot-b (Phase A substrate)
  - TASK-0260   # nx image writes the factory disk + signed NXBDs
  - TASK-0179   # Phase B backstop proofs run against the real apply engine
follow-up-tasks: []
kernel-touch: yes
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§5 NXBD, §7 loader, §10 anti-downgrade)
  - Boot-chain ADR (this task executes it): docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md
  - BSB actor discipline: docs/adr/0058-boot-selection-block-dual-actor-discipline.md
  - Authority: docs/adr/0055-bootctld-single-boot-state-authority.md
  - Testing contract: scripts/qemu-test.sh
---

## REWRITE 2026-08-25 (RFC-0089 lane recut — supersedes the whole pre-rewrite body)

The 2026-04-13 body named the goals (verified boot anchors, monotonic rollback,
measured handoff) without a mechanism. The mechanism is now contracted: a
first-stage loader (`nxboot`, ADR-0059) verifies a fixed 512-byte signed boot
descriptor (NXBD) + image digest + rollback floor BEFORE any OS code runs, reads
slot selection from the BSB projection (ADR-0058), and hands a measured record to
the kernel. The old depends-on chain (0007/0009/0029/0198) is replaced by the
lane's real substrate (0315/0260/0179); TASK-0037's bootargs ambition stays
absorbed here and is satisfied by the loader (no bootargs contract — selection
lives in the BSB). All prior invariants stand and are now enforceable:
anti-rollback is anchored in the boot path (not userspace-only), software root
is labeled `qemu-soft-root`, measurement is additive.

## Context

After TASK-0315 the disk is a single GPT image with real slot partitions, and
`nx image` (TASK-0260) writes signed NXBDs — but the VMM still loads
`neuron-boot.bin` directly: nothing selects, verifies, or falls back between
slots, so every A/B artifact describes state about a swap no code path performs.
This task builds the missing boot-time component and flips the boot path.

## Goal

**Phase A — `nxboot` + boot flip (the lane's flag-day package):**

- New top-level crate `source/boot/nxboot/` (bare-metal S-mode, no_std, no MMU;
  `#![forbid(unsafe_code)]` everywhere except ONE bounded early-asm module).
  Behavior per RFC-0089 §7: BSB read (double-block rule) → slot select with
  tries-decrement BEFORE load / exhaustion-flip → GPT walk (shared GUID table
  from `userspace/storage`, alloc-free or bounded arena) → NXBD + image load via
  its own minimal polling virtio-blk module → Ed25519 verify against build-baked
  `policies/os-trust.toml` (nxra BAKED_TRUST pattern; malformed file fails the
  build) → streamed sha256 → `rollback_index >= floor` → measured handoff page
  (ADR-0059 ABI) → jump with firmware registers restored. Loud fallback to the
  other slot on any verify failure; both slots bad ⇒ `nxboot: PANIC` + SBI reset
  (wait-loop doctrine: never hang).
- Machine logic (BSB codec, NXBD verify, slot-selection table) lives in
  host-testable modules; only entry/blk/uart are target-only.
- Kernel consumes the handoff page (probe magic + CRC; assert the page is outside
  early-managed ranges): present ⇒ expose via bootinfo,
  `KSELFTEST: boot handoff ok (measured)`; absent ⇒ honest
  `neuron: boot handoff absent (direct kernel)` (dev direct-kernel boots stay).
- Build/harness flip: `scripts/build.sh` builds `nxboot.bin` (linker.ld size
  assert ≤256 KiB + `check-image-budgets.sh` gate); `qemu-launcher.sh` boots
  `-kernel nxboot.bin` with the single `-drive nexus.img`; stale-disk detection
  (NXBD build-id vs. built kernel) refuses or patches explicitly. Reset-lane
  segmentation learns the per-boot nxboot rungs (markers.txt + docs together).

**Phase B — backstop proofs + measured surface (after TASK-0179):**

- Measured-boot userland surface: bootctld exposes the handoff record (decide:
  reuse caller-less `OP_GET_RECORD` op 11 or a dedicated op — RFC-0089 open
  question); `nx` debug read; label `measured (qemu-soft-root)` — never a
  hardware-root claim.
- Boot-time tamper backstop: fixture disk with corrupted boot-b payload behind a
  valid-shape NXBD ⇒ `nxboot: verify FAIL (slot=b digest)` → fallback boot A.
- Downgrade backstop: fixture with NXBD `rollback_index` below the BSB floor ⇒
  `nxboot: verify FAIL (slot=b rollback <n> < min <m>)` → fallback →
  `SELFTEST: ota downgrade deny ok` (loader rung; stage-time rung is 0179's).
- Tries-exhausted auto-fallback (`ota-fallback` profile, multi-boot): stage+switch
  a fault-fixture image that HONESTLY withholds health
  (`init: health withheld (fault fixture)`); boots decrement
  `nxboot: tries 2->1`, `1->0`, then `nxboot: fallback (slot=b exhausted) ->
  slot=a`; final boot: `bootctld: rollback observed` + `SELFTEST: ota fallback ok`.

## Non-Goals

- Hardware TEE/secure element, remote attestation, vendor ROM flows.
- The apply engine (TASK-0179), delta (0034/0035), flashing (0260 residual/0261).
- Any loader capability beyond the ADR-0059 frozen scope (filesystem, capnp,
  policy, network, target interpretation — all prohibited there).

## Constraints / invariants (hard requirements)

- **No security theater**: every marker and doc says `qemu-soft-root`; the
  hardware-anchor seam stays recorded here for the future.
- **Boot chain first**: the floor check in the loader is the anti-rollback
  authority backstop; userspace checks are conveniences, not the guarantee.
- **Deterministic denial**: stable FAIL reasons
  (`nxbd | sig | digest | rollback <n> < min <m> | io`), no fake-ready markers.
- Loader writes exactly the ADR-0058 actuator fields, nothing else, ever.
- Approval zones touched by Phase A: root `Cargo.toml`, `Makefile`, `scripts/**`,
  `source/kernel/**`, `config/**` (budgets) — PR series inside the package:
  crate + host tests first (buildable, unwired), the flip as ONE reviewed change.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- BSB codec matrix: seq/CRC/pick-newer/torn-block.
- NXBD vectors: accept, bad-sig, bad-digest, rollback-below-floor, zeroed.
- Slot-selection state table: trial decrement, exhaustion flip, both-slots-bad.
- Handoff-page encode/CRC golden; trust-bake build-failure test (malformed toml).

### Proof (OS / QEMU)

- Phase A: headless ladder led by `nxboot: bsb ok (slot=a seq=1)` →
  `nxboot: verify ok (slot=a build=<id8> rbidx=<n>)` → `nxboot: jump slot=a` →
  `KSELFTEST: boot handoff ok (measured)` → the ENTIRE existing ladder unchanged;
  reset three-boot lane green through the loader; keep-blk double boot green;
  SMP lanes green (loader runs boot-hart-only). Loader-fallback fixture lane:
  `nxboot: verify FAIL (slot=b nxbd)` → `nxboot: fallback -> slot=a` → full boot.
- Phase B: the three backstop lanes above + `SELFTEST: measured boot log ok`.

Fatal signatures registered in the harness: `nxboot: PANIC`, unexpected
`nxboot: verify FAIL` in clean lanes.

## Touched paths (allowlist)

- `source/boot/nxboot/` (new) + root `Cargo.toml` (approval)
- `source/kernel/neuron/` (handoff consumption — approval)
- `source/services/bootctld/` (measured surface, Phase B)
- `userspace/storage/` (shared GUID/GPT exports, no_std audit)
- `policies/os-trust.toml` (new)
- `scripts/build.sh`, `scripts/qemu-launcher.sh`, `scripts/qemu-test.sh`,
  `scripts/check-image-budgets.sh` (approval), `Makefile`/`justfile` (approval)
- `tools/nx/chains/markers.txt`, `source/apps/selftest-client/proof-manifest/`
- `docs/security/`, `docs/architecture/06-boot-and-bringup.md`

## Plan (small PRs)

1. **A1**: nxboot crate skeleton + host-tested machine modules (BSB/NXBD/select)
   + trust bake + linker/size gate — buildable, unwired.
2. **A2**: bare-metal blk reader + GPT walk + load/verify path; fixture-disk
   host-side integration test via `nx image`.
3. **A3**: kernel handoff consumption (approval) + KSELFTEST marker.
4. **A4**: THE FLIP — build/launcher/harness in one reviewed change; all lanes
   green; docs + memory-map audit note.
5. **B1**: measured surface + `SELFTEST: measured boot log ok`.
6. **B2**: backstop fixture lanes (`ota-fallback` profile, downgrade/tamper) +
   boards/docs sweep.

## Acceptance criteria (behavioral)

- The boot path itself proves trust decisions; downgraded or tampered images are
  rejected with stable reasons BEFORE any OS code runs, and the system still
  boots (fallback) or fails loudly (both bad) — never silently boots unverified
  bytes.
