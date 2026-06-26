# ADR-0038: One Rust SSOT for the windowd↔gpud display wire; Cap'n Proto stays the control plane

- Status: Accepted (landed: `source/libs/nexus-display-proto`; gpud + windowd import it — Gate 2 of the gfx/driver idealstruktur track).
- Created: 2026-06-26
- Builds on: ADR-0028 (windowd surface/present), ADR-0032 (GPU command ring), RFC-0059 (NexusGfx SDK + GPU driver contract).
- Code: `source/libs/nexus-display-proto`, `source/drivers/gpud/src/service.rs`, `source/services/windowd/src/compositor/runtime/{mod,cursor}.rs`. Descriptive schemas: `source/drivers/gpud/idl/gpud.capnp`, `source/services/windowd/idl/surface.capnp`.

## Context

The windowd↔gpud wire had **three partial descriptions** and no single owner:

1. The hot per-frame payload is the `nexus_gfx::CommittedBuffer` codec (already an SSOT).
2. The thin control frames (opcodes, status codes, cursor-reply magics, the attach / legacy-damage
   frames, handoff-id decode) were **hand-defined twice** — gpud's `OP_*` / `STATUS_*` in
   `service.rs` and windowd's `GPU_*_OP` / `GPUD_STATUS_*` / `encode_gpud_*`, the latter literally
   commented "`mirrors gpud::OP_*`". The two crates share no dependency, so the values were kept in
   sync by hand and could drift silently.
3. A pair of `.capnp` "seed" schemas (`gpud.capnp`, `surface.capnp`) described the wire but are **not
   code-generated** — they generate nothing and only documented intent, a fourth thing to drift.

Separately, the system's **control plane** (samgr, policyd, bundlemgr, vfs, …) already uses Cap'n
Proto. The open question raised on this track: should the data-plane wire also move to Cap'n Proto
for uniformity, or stay hand-rolled? The decision had to be made on measured properties, not taste.

`gpud/src/protocol.rs` is a **different layer** — the virtio-gpu *device* protocol (MMIO offsets +
spec command structs) — and is out of scope here; it belongs to the future `nexus-virtio` HAL.

## Decision

**The windowd↔gpud wire is owned by one small Rust crate, `nexus-display-proto`.** It defines the
opcodes, status codes, cursor-reply magics, and the control-frame encoders/decoders **once**; both
gpud and windowd depend on it. The historical local names are kept but now re-source the crate's
values (e.g. `const GPU_PRESENT_DAMAGE_OP: u8 = nexus_display_proto::OP_PRESENT_DAMAGE;`), so there is
no call-site churn and the bytes are unchanged — purely a single-source-of-truth move.

**The hot per-frame stream stays hand-rolled** (opcode byte + serialized `CommittedBuffer`), not
Cap'n Proto. Rationale, verified rather than assumed:

- These are tiny, fixed, little-endian frames on the boot/handoff and per-frame paths. Cap'n Proto's
  segment table + struct/list pointers + 8-byte word alignment make the encoded message **larger**
  than the few packed LE fields, which fights the IPC frame budget for the command stream.
- Cap'n Proto's wins (zero-copy reads of large/nested/optional-heavy messages, cross-language schema
  evolution) do not apply to this small, flat, single-language boundary.
- The bulk pixel data is **already zero-copy** via the shared framebuffer VMO (capability move); no
  serialization touches it. So Cap'n Proto would add cost here with no copy-avoidance benefit.

**Cap'n Proto remains the control-plane choice.** Where messages are larger, rarer, and benefit from
schema evolution (the system services above), Cap'n Proto stays. The principle: *uniform structure
where performance is equal; the right tool where it is not* — here the hot data-plane wire measurably
favors the packed Rust SSOT.

**The `.capnp` seeds are demoted to descriptive documentation.** Their headers now say
"DESCRIPTIVE ONLY — not code-generated" and point at `nexus-display-proto` as the SSOT, so no one
mistakes a non-compiled schema for the contract.

## Consequences

- One definition per wire constant and control frame; the "mirrors gpud::" hand-sync is gone and a
  mismatch is now a compile error across the shared crate, not a silent runtime drift.
- Zero wire-byte change → no protocol/boot risk; verified host (gpud, windowd) + riscv (virgl + 2D)
  green and boot-confirmed.
- A future option, explicitly not taken now: moving only the **control handshakes** (attach / cursor
  upload) to Cap'n Proto would be performance-neutral (they are small and rare) but adds a Cap'n
  Proto dependency to gpud (which has none) and touches the boot-critical handoff; the Rust SSOT
  already delivers the uniformity goal with less risk.
