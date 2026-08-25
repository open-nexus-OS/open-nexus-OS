# ADR-0058: Boot Selection Block (BSB) — derived projection with a field-level dual-actor write matrix

- Status: Accepted
- Date: 2026-08-25
- Links:
  - RFCs: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md`
    (§6 BSB format — normative layout lives there)
  - Tasks: `tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md`
    (Phase B: bootctld projection writer),
    `tasks/TASK-0289-boot-trust-floor-v1-verified-boot-rollback-measurement.md`
    (Phase A: `nxboot` boot-time actuator),
    `tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md`
    (factory initialization via `nx image build`)
  - Related ADRs: `docs/adr/0055-bootctld-single-boot-state-authority.md` (the
    authority this projects), `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md`
    (the `bsb` partition substrate)

## Context

The first-stage loader (`nxboot`, RFC-0089 §7) must read boot-selection state before
any OS code runs — but the authoritative boot record is a statefs Integrity envelope
(`/state/boot/bootctl.v1`, ADR-0055), which only exists after journal replay inside
statefsd. Teaching a bare-metal trust root to replay a statefs journal is out of the
question (size, surface, write-path entanglement). Conversely, moving the authority
into a raw sector would re-open the multi-owner drift ADR-0055 just closed.

Additionally, A/B correctness requires a boot-time actor: the tries decrement must
happen BEFORE the (possibly broken) image runs, or a boot-loop into a wedged image
never converges to rollback.

## Decision

- The `bsb` partition holds a **derived projection** of the bootctld record — the same
  doctrine as `policy.bin` (derived; authority stays with the daemon). The bootctld
  statefs record remains the single boot-state authority (ADR-0055 unchanged).
- **Double-block discipline**: two 512-byte blocks; readers take the valid
  (magic+CRC) block with the higher `seq`; writers always write the OTHER block.
  One-sector writes are power-cut atomic on the virtio-blk substrate; a torn write
  leaves the previous block authoritative. Layout is normative in RFC-0089 §6.
- **Field-level write matrix** (exhaustive — any write outside this matrix is a
  contract violation):

  | actor | when | may write |
  |---|---|---|
  | `bootctld` | runtime, after every committed record mutation | ALL fields (full re-projection; `seq+1`) |
  | `nxboot` | boot time, before loading a trial slot | `tries_left` (decrement only); on exhaustion additionally `active_slot` ← rollback target, `next_slot` ← none (`seq+1`) |
  | `nx image` | factory / provisioning | full initialization (`seq=1`) |

- **Ordering discipline**: bootctld commits the statefs record FIRST (authority),
  projects to BSB SECOND. A crash between the two is healed by idempotent
  re-projection at bootctld startup (`bootctld: bsb resync` when the projection
  differed). The projection is a pure function of the record — no BSB-only state.
- **Convergence rule**: at startup bootctld reconciles boot-time actuator writes
  (tries decrements, exhaustion flips) INTO the record via the existing
  `OP_BOOT_ATTEMPT`/rollback machine semantics before the first re-projection —
  actuator effects are absorbed by the authority, never overwritten blindly.
- The loader treats `boot_target` as opaque pass-through (RFC-0087 §4 semantics stay
  entirely inside bootctld/init); the loader never interprets targets.

## Consequences

- **Positive**: the loader reads one CRC-checked sector pair — no filesystem, no
  journal replay, bounded code; single authority preserved; power-cut behavior is
  provable with a two-line torn-write test matrix; tries-before-load makes wedge
  loops converge without any OS cooperation.
- **Negative / accepted cost**: two actors write one artifact — accepted because the
  matrix is field-disjoint per phase (runtime vs. boot time never overlap in time on
  one device) and reconciliation is absorbed through the existing machine ops; a
  crash between record commit and projection leaves one boot on the stale
  projection (bounded staleness: exactly the mutations of one commit; healed at next
  bootctld start, and the loader-relevant fields — slot/tries — only change through
  flows that end in a reset anyway).
- **Follow-ups**: TASK-0036-B (projection writer + resync), TASK-0289-A (actuator),
  TASK-0260 (factory init), harness reset-lane segmentation learns the nxboot rungs.

## Alternatives considered

- **Loader replays statefs**: rejected — trust-root surface explosion; couples the
  loader to a journal format that evolves.
- **Move the authority into the BSB (raw sector as SSOT)**: rejected — loses the
  Integrity envelope, monotonic seq audit and snapshot-restore discipline; re-opens
  ADR-0055's multi-owner problem for every consumer that is not the loader.
- **Loader does not write (OS-side tries only)**: rejected — a boot loop into an
  image that never reaches userspace would decrement nothing and retry forever;
  the boot-time decrement is the convergence guarantee.
- **A/B copies of the whole record instead of a projection**: rejected — the record
  is envelope-wrapped and schema-evolving; the loader needs five stable fields, not
  a schema dependency.
