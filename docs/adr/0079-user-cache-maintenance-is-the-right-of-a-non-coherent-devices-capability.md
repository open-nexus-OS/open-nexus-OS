# ADR-0079: User-mode cache maintenance is the right of a non-coherent device's capability — the kernel enables it per process, on the first use, never for everyone

- Status: Accepted (2026-10-25, board cycle 69: blkd, xhcid and gpud granted once each, the refusal proven on QEMU and the board)
- Date: 2026-10-23
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6c-1a — before any code page is shared)
  - RFCs: `docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md` (C4: coherence in the device capability, `DmaBuffer`), `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md` (6c: shared code)
  - Related ADRs: `docs/adr/0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md`

## Context

RFC-0098 C4 enabled Zicbom for user mode on every hart (`senvcfg.CBCFE = 1`, `CBIE = 01`), for
every task alike, so a driver of a device that does not snoop the CPU caches can write back and
drop the lines of its DMA buffers. A cache-block instruction is permitted wherever a load is
(the CMO specification): any task can therefore flush a line of memory it can merely read. Two
tasks reading the same physical page are the precondition of a Flush+Reload probe — flush a line,
wait, time the reload, learn whether the other task touched it. That precondition exists today:
the glyph atlas is one page set read by every app-host (RFC-0080), so an app could, in
principle, observe which glyphs another app draws; 6c makes code pages shared as well. Whether
the board's timer resolves the reload is not measured; the primitive is there either way.

Who needs the instruction, measured 2026-10-23: one call site in user mode
(`nexus_abi::cache_clean` / `cache_flush`), reached only through `nexus_driverkit::DmaBuffer` /
`DmaShared` — the drivers of the board's non-coherent devices (SDHCI, xHCI, the display
controller in gpud) and the selftest's DMA probe. Every one of them holds the capability of a
device; the capability already records whether the device snoops (`DeviceMmio {
dma_noncoherent }`, RFC-0098 C4).

## Decision

- **The right is the capability.** A process may execute `cbo.clean` / `cbo.flush` / `cbo.inval`
  (as a flush) only once a capability of a **non-coherent** device (`DeviceMmio {
  dma_noncoherent: true }`) is in its table. Nothing else grants it: no manifest key, no policy
  row, no syscall. A future driver of a non-coherent device (Ethernet, audio, camera, codec,
  NPU, crypto) gets it with the capability init hands it — nothing per driver. A driver of a
  coherent device (every virtio device on QEMU) does not need it and does not get it.
- **Off by default, on at the first use (the lazy pattern of an FPU).** Every hart boots with
  `CBCFE = 0`, `CBIE = 00`. A cache-block instruction from user mode then traps as an illegal
  instruction; the kernel decodes it (`MISC-MEM`, `funct3 = 010`, `rd = 0`, `imm` 0/1/2), and
  if the current process holds the right, marks its address space, enables the bits on this
  hart and retries the instruction. Without the right the trap is the fault it was — the
  process dies with `reason=fault`, cause IllegalInstruction.
- **Per process, at the switch.** An address space that was granted keeps the right for its life
  (a driver stays a driver; the capability may not be named again); activating an address space
  writes `senvcfg` only when the hart's bits differ from what that space may do. A process
  without the right never runs with the bits on.
- **Non-drivers never get the instruction.** When something that is not a driver must make its
  writes visible to a non-coherent device (layer 8's zero-copy import from an app), the kernel
  performs the maintenance in a call over an object the caller may **write** — never over one it
  can only read — so no shared read-only page (code, the atlas) can be flushed by an app. That
  call is built when its first user exists, not now.
- `cbo.zero` (Zicboz, `CBZE`) stays off for user mode; it is out of scope.

## Consequences

- **Positive**: the Flush+Reload primitive is gone for every app before any code page is shared;
  the atlas exposure of RFC-0080 closes with it; the kernel stays policy-free (the right is an
  object a process holds); future drivers need no configuration.
- **Negative / accepted cost**: one trap per process the first time it maintains a line after a
  switch onto a hart whose bits were off (a CSR write and a retry); the selftest's DMA probe can
  no longer force maintenance on QEMU's coherent device — it follows the device's own coherence,
  and the board proves the positive path (its drivers maintain or their DMA breaks visibly);
  a child without the right proves the refusal on every boot.
- **Follow-ups**: the kernel maintenance call for non-drivers (layer 8, with its first user);
  a measurement of the board's reload timing (only to quantify, the decision does not depend
  on it).

## Alternatives considered

- **Keep it global** (RFC-0098 C4 as written): rejected — every app holds a primitive against
  every page it shares, and 6c widens what it shares.
- **Grant it to every holder of any device capability**: simpler, keeps the QEMU probe as it was;
  rejected — a coherent device's driver has no use for it (least privilege), and the board's
  drivers prove the positive path anyway.
- **Set the right when the capability is inserted** (every grant, transfer, CAP_MOVE path):
  rejected — many insertion paths, and a capability arriving at a running task on another hart
  would race the bits; the lazy trap is correct whenever the capability arrived.
- **A manifest declaration through policyd**: rejected — the capability already is the
  authority; a second one could disagree with it.
- **Non-cacheable mappings (Svpbmt) instead of maintenance**: the fallback RFC-0098 names for a
  board without Zicbom; slower for buffers the CPU fills, and not needed on this board.
