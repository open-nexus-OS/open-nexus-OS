---
title: TASK-0289 Boot trust floor v1: nxboot first-stage loader + boot flip (Phase A) and backstop proofs + measured surface (Phase B)
status: Done 2026-08-31 — Phase A (loader + flip) 2026-08-30, Phase B (measured surface + three backstop lanes) 2026-08-31; all gated in test-all
owner: @security @runtime @updates
created: 2026-04-13
updated: 2026-08-31
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

## Progress

- **A1 DELIVERED 2026-08-30**: `source/boot/nxboot/` (workspace member; lib =
  host-tested machine half, bin = bare-metal shell). `bootfmt::handoff` codec
  (ADR-0059 page ABI v1, golden/tamper/prefix-decode tests; crc32 shared with
  bsb). `select::plan()` — full state table host-tested (trial decrement
  BEFORE load, exhaustion clears ONLY next_slot per ADR-0058, seq saturates).
  `policies/os-trust.toml` → build-baked `BAKED_OS_KEYS` via the SHARED narrow
  parser (`userspace/updates/build_trust.rs`); integration tests prove
  bake ↔ policy-file ↔ dev-signing-seed coherence + tamper/stranger-key
  rejects. `linker.ld`: 16 KiB stack, HARD ASSERT ≤256 KiB; build.rs
  self-wires `-T linker.ld` for riscv/none; root profile `opt-level="z"`.
  The ONE unsafe module is `src/arch.rs` (entry asm + uart MMIO + SRST +
  the `no_mangle` export; `no_mangle` counts as unsafe-code surface, hence
  it lives there).
- **A2 DELIVERED 2026-08-30** — the loader is COMPLETE and smoke-proven:
  - ⭐ **Memory-map audit (ADR-0059 first stop condition) MOVED the
    constants**: the 2026-08-25 proposals (home 0x8E00_0000, handoff
    0x8FE0_0000) fell INSIDE the kernel's user VMO arena
    (0x8380_0000..0x9180_0000, RAM 320 MiB). Frozen values live in the
    amended ADR: home **0x9200_0000**, handoff **0x9300_0000** — both in
    the 40 MiB band above the arena that no kernel range manages
    (`bootfmt::handoff::ADDR` updated).
  - `flow::run()` — the ENTIRE pipeline generic over `storage::BlockDevice`
    (BSB double block → select/actuate BEFORE load → GPT walk via the
    shared authority → NXBD verify → bounds/load_addr → rollback floor →
    image read → streamed digest → other-slot fallback), host-proven in
    tests/loader_flow.rs against MemBlockDevice fixture disks assembled
    with the SAME shared functions `nx image build` calls (layout::plan +
    write_gpt + factory BSB + NXBD-last): clean boot, 4-boot trial/
    exhaustion ladder, tamper→digest, downgrade→rollback backstop,
    stranger-key→sig, zeroed-NXBD→nxbd, wrong load_addr→nxbd, both-bad
    terminal, torn-BSB-pair→never guess.
  - Target half: self-relocation asm (PC-relative until the computed
    absolute jump home), bss/stack bring-up, 192 KiB bounded bump
    allocator (OOM = loud PANIC + reset), own polling virtio-blk reader
    (MODERN transport only — force-legacy=off is the harness reality; a
    legacy window is refused loudly, never half-driven; bounded poll
    spins), handoff-page write, `fence.i` + jump with a0/a1 restored.
  - ⭐ **QEMU smoke proof (manual, pre-flip)**: `-kernel nxboot.bin -drive
    nexus.img` boots the FULL OS through the loader — `nxboot: bsb ok
    (slot=a seq=1)` → `nxboot: verify ok (slot=a build=dev-a7c6 rbidx=0)`
    → `nxboot: jump slot=a` → kmain → init service ladder. Loader binary:
    55 KiB flat (22 % of budget). Failure modes observed live: PANIC+SBI-
    reset loop is deterministic (1 panic per OpenSBI banner, single-hart
    entry confirmed — OpenSBI parks secondaries).
  - TRAP for A4: plain `-machine virt` gives LEGACY virtio-mmio (poll
    timeout); the launcher's `-global virtio-mmio.force-legacy=off` is
    what selects modern — any new lane must inherit that flag.
- **A3 DELIVERED 2026-08-30** — kernel handoff consumption:
  `neuron::boot_handoff` (core/boot_handoff.rs): capture runs FIRST THING
  in kmain, pre-SATP on the boot hart — the page is deliberately outside
  every kernel-managed range and therefore unreachable after the address
  space activates (const-asserted above the arena end + inside RAM).
  Validated copy in a kernel static, `get()` accessor ready for the B1
  bootctld surface. Three honest verdicts: present ⇒ `KSELFTEST: boot
  handoff ok (measured)` + `neuron: boot handoff slot=<s> rbidx=<n>
  seq=<n> (qemu-soft-root)`; magic-without-valid-CRC ⇒ named invalid,
  treated as absent; absent ⇒ `neuron: boot handoff absent (direct
  kernel)`. Kernel-side decode is a deliberate 60-byte parity twin of
  `bootfmt::handoff` (kernel cannot link dalek); the measured rung in the
  ladder IS the cross-implementation drift gate. PROVEN both ways:
  headless ladder green with the absent marker (direct kernel), manual
  nxboot boot against the fresh disk shows the measured pair.

- **B1 DELIVERED 2026-08-31** — the measured surface, headless-proven
  (`SELFTEST: measured boot log ok` REQUIRED in the ladder):
  - Kernel `SYSCALL_BOOT_HANDOFF` (57): copies the RAW validated 60-byte
    record into a caller buffer (`len >= 60` enforced; absent/corrupt ⇒ 0
    — measurement is never fabricated). The kernel keeps the raw page
    bytes alongside the decoded copy so userland re-decodes with the SAME
    host-tested `bootfmt` codec instead of a third parity twin.
  - bootctld `OP_GET_MEASURED` (12) — resolves the RFC-0089 open question
    with a DEDICATED op (`OP_GET_RECORD` stays the persisted-record
    snapshot: different authority, lifetime, payload). Payload =
    `[present u8] + raw record`; read once at attach; served verbatim.
    Reply plumbing hardened on the way: bootctld's reply buffer was a
    silent-truncating `[u8; 32]` (the P8 additive-tail family — a 61-byte
    payload would have starved every prefix check); now `RSP_LEN = 96`
    with the trap documented at `encode_payload`.
  - Selftest cross-check probed at OTA-PHASE START: measured slot ==
    authority active slot. ⭐ FOUND BY THE PROBE ITSELF: probed at phase
    END it compared boot evidence against post-OTA state (the phase's
    stage/switch legitimately moves the active slot without a reboot) and
    failed honestly — boot-time claims must be checked before the state
    machine moves. Diagnostic `SELFTEST: measured dbg …` line kept.
  - ADR-0059 amendment documents the two-hop surface + the no-capability
    rationale (public boot evidence, printed on the uart anyway).

- **B2 DELIVERED 2026-08-31 — the three backstop lanes, all green and
  gated (`just ci-os-ota-backstops` in `test-all`)**:
  - **`nx image backstop --kind tamper|downgrade`** arms a built disk:
    plants boot-b behind a fully VALID signed NXBD (tamper flips ONE
    payload byte AFTER the descriptor landed; downgrade is fully valid at
    rollback index 0 under the factory floor 1) and arms the BSB
    `next=b tries=2` via the alternate-block writer rule (CLI test proves
    the arm + floor survival; the rejects themselves are host-proven in
    nxboot tests/loader_flow.rs since A2).
  - **`ota-tamper` lane**: `bsb ok (slot=b)` → `tries 2->1` →
    `verify FAIL (slot=b digest)` → `fallback -> slot=a` → the FULL
    headless ladder incl. `bootctld: bsb resync` (the record authority
    clears the planted trial) and the measured cross-check.
  - **`ota-downgrade` lane**: same shape,
    `verify FAIL (slot=b rollback 0 < min 1)` — the loader floor check as
    the anti-downgrade backstop, before any OS code runs.
  - **`ota-fallback` lane (four boots, ONE uart)** — the crown of Phase B:
    boot 1 stages the REAL os-B + switch (record and BSB agree, same
    arming as the flip lane); boots 2/3 are HONESTLY dead — init parks at
    `init: health withheld (fault fixture)` BEFORE any service spawns
    (nexus-init `fault_fixture.rs`: trial detection from the loader's own
    measured record, the knob from fw_cfg `selftest-profile`), because a
    live userspace would shadow the proof (the boot-attempt tick exhausts
    the RECORD first and bootctld rolls back in software). The harness
    plays the power-cycle role (`tools/qmp_reset_on_marker.py`, one QMP
    system_reset per NEW withheld marker, bounded). The loader alone
    walks `tries 2->1`, `1->0`, then
    `nxboot: fallback (slot=b exhausted) -> slot=a` (BSB seq 3→4→5, all
    actuator writes); boot 4: `bootctld: rollback observed (trial
    exhausted)` + `SELFTEST: ota fallback ok`.
  - **Rollback observation (bootctld)**: at attach, an on-disk pair
    showing the actuator's exhaustion shape (`next=None`, `tries=0`,
    `health_committed=false`) against a still-pending record rolls the
    RECORD back (persist + loud marker) instead of re-projecting — which
    would hand the broken image its tries back. `health_committed` is the
    discriminator against the commit→projection crash window (that block
    is a committed steady state and re-projects as before); predicate
    `bsb::exhaustion_observed` + the four-shape host test in
    tests/bsb_projection.rs. Normative in RFC-0089 §6.
  - **Shared fw_cfg reader** `nexus_abi::fwcfg` (bounded file-dir walk;
    parity source: selftest boot_cfg.rs — dedup of the selftest copy
    deferred, noted here).
  - **Deliberate scope**: the ledger's `nx` debug read of the measured
    record is covered by the uart line (`neuron: boot handoff …
    qemu-soft-root`) + `OP_GET_MEASURED`; a host-side read path would
    need a guest transport that does not exist — revisit with TASK-0140's
    CLI if wanted.
  - ⭐ Session traps worth keeping: TWO cfg-attribute thefts (a `mod` line
    inserted between `#[cfg(...)]` and its module steals the attribute —
    broke nexus-abi host builds and bootctld host tests, found both
    times by gates); NEVER edit a harness/launcher shell script while a
    lane executes it (bash reads incrementally — the downgrade lane died
    on a mid-run rewrite at a stale byte offset).

## Plan (small PRs)

1. **A1**: nxboot crate skeleton + host-tested machine modules (BSB/NXBD/select)
   + trust bake + linker/size gate — buildable, unwired. ✅ 2026-08-30
2. **A2**: bare-metal blk reader + GPT walk + load/verify path; fixture-disk
   host-side integration test via `nx image`. ✅ 2026-08-30 (fixture disks
   assembled with the shared layout/gpt/bootfmt authorities nx image calls)
3. **A3**: kernel handoff consumption (approval) + KSELFTEST marker. ✅ 2026-08-30
4. **A4**: THE FLIP ✅ 2026-08-30 — build/launcher/harness in one change:
   - `scripts/build.sh` builds nxboot (RUSTFLAGS_OS, artifact-verified);
     `qemu-launcher.sh` boots `-kernel nxboot.bin` (shared `objcopy_flat`
     helper; `NEXUS_DIRECT_KERNEL=1` keeps honest direct-kernel dev boots).
   - `qemu-test.sh`: nxboot rungs + `KSELFTEST: boot handoff ok (measured)`
     PREPENDED to expected_sequence for every profile (after the SMP
     splice — the `:0:7` index math must not shift; the handoff marker is
     emitted at the very top of kmain so strict KSELFTEST ordering holds
     in SMP lanes too). Fatal guards: `nxboot: PANIC` and `nxboot: verify
     FAIL` in any clean lane. Proof-manifest bringup.toml carries the four
     markers (mirror-check).
   - `check-image-budgets.sh`: nxboot 256 KiB loadable-bytes budget row
     (text+data; linker.ld asserts the same bound at link time).
   - Proofs: headless ladder REQUIRED-green through the loader; keep-blk
     double boot; reset three-boot lane (3× `nxboot: jump` in one uart);
     fallback fixture disk (BSB next=b onto the zeroed slot) shows
     `tries 1->0` → `verify FAIL (slot=b nxbd)` → `fallback -> slot=a` →
     full boot with seq bumped by the actuator write.
   - ⭐ Observed flake (1×, rerun green): single dropped uart CHARACTER in
     the reset lane turned `statefsd: fsck repaired` into `tatefsd: …` —
     marker emitted, byte lost on the wire. If it recurs, suspect the
     serial monitor pipeline, not the services.
(A4 delivered above.)
5. **B1**: measured surface + `SELFTEST: measured boot log ok`. ✅ 2026-08-31
6. **B2**: backstop fixture lanes (`ota-fallback` profile, downgrade/tamper) +
   boards/docs sweep. ✅ 2026-08-31

## Acceptance criteria (behavioral)

- The boot path itself proves trust decisions; downgraded or tampered images are
  rejected with stable reasons BEFORE any OS code runs, and the system still
  boots (fallback) or fails loudly (both bad) — never silently boots unverified
  bytes.
