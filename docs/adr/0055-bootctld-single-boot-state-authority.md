# ADR-0055: `bootctld` is the single boot-state authority; `updated` becomes a client

- Status: Accepted
- Date: 2026-08-18
- Links:
  - Tasks: `tasks/TASK-0050-system-reset-boot-targets-bootctld.md` (execution + proof),
    `tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md` (client rebase),
    `tasks/TASK-0178-bootctld-v1-boot-control-stub-service.md` (absorbed → Superseded)
  - RFCs: `docs/rfcs/RFC-0087-reliability-failure-model-v1.md` (boot-target semantics),
    `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (slot machine origin)
  - Related ADRs: `docs/adr/0024-updates-ab-packaging-architecture.md` (superseded on
    record ownership only — the slot semantics stand)

## Context

Boot state — boot target, next-boot selector, active slot, rollback index, boot
attempts — currently has four competing candidate owners across ledgers and code:
`updated` owns the shipped record (`/state/boot/bootctl.v1`), TASK-0050 planned a
`nexus.target` parser in init, TASK-0261 planned a `rebootd` with its own
`state:/boot/next_mode` key, and TASK-0178/0289 name a `bootctld`. Two boot-arg
contracts (`nexus.target=` vs `recovery=1`) exist on paper. An update daemon owning
boot targets is a responsibility mismatch that TASK-0289 (verified boot, rollback
indices) would have to unwind later at higher cost.

## Decision

- **`bootctld` is the single authority** for the boot-state record: boot target
  (`normal | recovery | safe`), next-boot (one-shot), active slot, `tries_left`,
  rollback index, boot-attempt counter.
- The record stays one statefs Integrity-envelope record (monotonic seq) at
  `/state/boot/bootctl.v1`; the proven state machine
  (`userspace/updates/src/bootctrl.rs`) and persistence
  (`source/services/updated/src/bootctl_state.rs`) **move** to `bootctld` — they
  are not reimplemented.
- `updated` becomes a `bootctld` client for stage/switch/health/rollback;
  `nexus-init` reads the record at boot and consumes `next_boot` exactly once.
- **Retired by this decision**: `rebootd` (TASK-0261), the independent
  `state:/boot/next_mode` key, the `recovery=1` boot arg, and any `nexus.target`
  parsing in init. Reset itself (SBI SRST) is a kernel primitive invoked via
  `bootctld`.
- Out of scope: verified-boot anchors and measured boot (TASK-0289 builds *on*
  this record), update feed/delta formats (TASK-0034/0035/0179).

## Consequences

- **Positive**: one owner for the state TASK-0289 must trust; recovery/safe boot
  and OTA rollback share one record and one proof (keep-blk double boot); the
  authority registry gets a real entry instead of four candidates.
- **Negative / accepted cost**: moving proven code out of `updated` (small,
  mechanical — the QEMU OTA ladder must stay green through the move);
  `TRACK-AUTHORITY-NAMING.md` and ADR-0024 need index/ownership updates;
  TASK-0178 becomes Superseded, TASK-0261 loses `rebootd`.
- **Follow-ups**: TASK-0050 (execution), paper rebase of 0036/0178/0179/0261/0289.

## Alternatives considered

- **Extend `updated`** (least motion; record already lives there): rejected — an
  update service owning boot targets is a naming/authority blur that 0289 reopens.
- **`nexus-init` owns it**: rejected — init is not a daemon; runtime transitions
  (`nx reboot recovery`) would have no addressee.
- **Keep `rebootd` as separate next-boot selector**: rejected — a second writer to
  the same decision is the drift this ADR exists to kill.
