# ADR-0059: First-stage boot chain — `nxboot` loader position, self-relocation, measured-boot handoff ABI

- Status: Accepted
- Date: 2026-08-25
- Links:
  - RFCs: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md`
    (§5 NXBD, §7 loader behavior — normative), `docs/rfcs/RFC-0087-reliability-failure-model-v1.md`
    (targets untouched)
  - Tasks: `tasks/TASK-0289-boot-trust-floor-v1-verified-boot-rollback-measurement.md`
    (execution + proof), `tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md`
    (disk substrate)
  - Related ADRs: `docs/adr/0058-boot-selection-block-dual-actor-discipline.md` (BSB),
    `docs/adr/0056-task-exit-reason-kernel-abi.md` (precedent: kernel↔userspace ABI gate)

## Context

Verified A/B boot needs a component that runs BEFORE the OS image, selects a slot,
verifies it, and can fall back. Today the VMM loads `neuron-boot.bin` directly (the
`-kernel` option); there is no boot-time chooser and no verification. This ADR fixes
the loader's position in the chain, its memory contract, and the loader→kernel
handoff ABI — an architecture-boundary crossing (boot↔kernel) per the repo's
arch-gate rules.

## Decision

- **Chain**: SBI firmware (`-bios` default) → **`nxboot`** (new top-level crate
  `source/boot/nxboot/`, bare-metal S-mode, no_std, no MMU) → verified boot image
  (today's `neuron-boot.bin`, unchanged entry contract). The VMM `-kernel` payload
  becomes `nxboot.bin`; direct-kernel dev boots remain possible (see handoff-absent
  rule).
- **Memory contract** (constants recorded here; audited against the kernel memory
  map before freeze — TASK-0289-A's first stop condition):
  - Firmware enters `nxboot` at the standard supervisor entry (0x8020_0000) with
    `a0 = hartid`, `a1 = DTB` — preserved across relocation, restored before jump.
  - `nxboot` self-relocates to a high home (proposal 0x8E00_0000) so it can load the
    image to 0x8020_0000 without overlap; position-independent early asm isolated in
    ONE bounded module (the only `unsafe`-bearing file; everything else
    `#![forbid(unsafe_code)]`).
  - Handoff page at 0x8FE0_0000 (inside guest RAM, outside kernel early-managed
    ranges — kernel asserts this at handoff consumption).
  - Only the boot hart runs `nxboot`; secondary harts stay parked in SBI HSM (the
    kernel starts them exactly as today — the SMP lanes are the regression signal).
- **Loader scope is frozen** (anti-drift): BSB read/actuate, GPT walk, NXBD+image
  load/verify, handoff write, jump. NO filesystem, NO capnp, NO policy, NO network,
  NO interpretation of boot targets. Growth beyond this list requires a new ADR.
- **Trust bake**: `policies/os-trust.toml` → build-script codegen → baked verifying
  key set in `nxboot` (RFC-0088 `BAKED_TRUST` pattern; malformed trust file fails
  the build). Honest label: QEMU-soft-root until a hardware anchor exists.
- **Size budget**: `nxboot.bin` ≤ 256 KiB, asserted in the linker script and in
  `scripts/check-image-budgets.sh`. Panic behavior: marker `nxboot: PANIC ...` +
  SBI system reset — never a silent hang (wait-loop doctrine).
- **Measured-boot handoff ABI (v1, normative)** — one 4 KiB page:

  ```text
  [0..8)    magic "NXHO0001"
  [8..10)   version u16 (=1)
  [10..11)  boot_slot u8 (0=a, 1=b)
  [11..12)  tries_decremented u8 (0|1)
  [12..16)  rollback_index u32 (of the booted NXBD)
  [16..48)  image_sha256 [32]
  [48..56)  bsb_seq u64
  [56..60)  crc32 over [0..56)
  [60..4096) reserved (0)
  ```

  The kernel probes the magic: present+CRC-valid ⇒ expose the record read-only via
  bootinfo to userspace (bootctld is the surface owner — no new daemon); absent ⇒
  the kernel reports `neuron: boot handoff absent (direct kernel)` — honest, never a
  fake measured claim. The record is per-boot, immutable after handoff, and carries
  MEASUREMENT (what booted), never policy.

## Consequences

- **Positive**: verified boot + anti-rollback backstop + measured boot land as ONE
  small auditable component; the boot image and its entry contract stay unchanged
  (the kernel does not learn to verify itself); dev velocity keeps direct-kernel
  boots with an honest `handoff absent` label.
- **Negative / accepted cost**: a second bare-metal environment to maintain (own
  linker script, own blk reader — the userspace driver stack is syscall-bound and
  not reusable pre-OS); the boot flip is a flag-day change across four approval
  zones (mitigated: crate+host tests land first, the flip is one reviewed change);
  disk-boot adds seconds of poll-read to every QEMU lane (timeout budgets adjusted
  with the flip).
- **Follow-ups**: TASK-0289-A (loader + flip), TASK-0289-B (measured surface +
  backstop proofs), harness segmentation for nxboot rungs, memory-map audit note in
  the kernel docs.

## Alternatives considered

- **Kernel self-verifies (no loader)**: rejected — the artifact being verified would
  verify itself; rollback/fallback before the image runs is impossible.
- **Firmware-level verification (custom SBI payload)**: rejected — forks the
  firmware, couples us to its build; the S-mode loader is ours end-to-end and
  QEMU-portable.
- **Loader in M-mode replacing the firmware**: rejected — reinvents SBI (HSM, SRST,
  DBCN) for zero trust gain in the QEMU-soft-root era.
- **DTB/cmdline handoff instead of a fixed page**: rejected — mutating the DTB in a
  bare-metal loader adds a parser/writer for marginal benefit; a CRC'd fixed page is
  bounded and testable.
