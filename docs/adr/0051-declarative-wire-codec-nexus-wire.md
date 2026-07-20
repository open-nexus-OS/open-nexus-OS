# ADR-0051: Service wire frames are declared, not hand-coded — one codec SSOT crate `nexus-wire`

- Status: Accepted (landed 2026-07-20: `source/libs/nexus-wire` + nexus-abi shims + syscall split; gates green — `just check`, `test-host`, `dep-gate`, `diag` host+os+kernel, `ci-os-smp1` boot proof)
- Date: 2026-07-20
- Links:
  - Tasks: `tasks/TASK-0296-nexus-wire-declarative-service-frame-codec.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (nonce correlation contract the engine encapsulates)
  - Related ADRs: `docs/adr/0038-display-wire-ssot-and-capnp-boundary.md` (precedent: packed-LE Rust SSOT for hot wires, Cap'n Proto stays control plane), `docs/adr/0016-kernel-libs-architecture.md` (nexus-abi charter)

## Context

`source/libs/nexus-abi/src/lib.rs` (4103 LOC, the largest grandfathered entry in
`config/loc-baseline.txt`) conflates two unrelated layers:

1. **Kernel↔userspace ABI** — ecall wrappers, syscall IDs, `MsgHeader`,
   `IpcRecvV2Desc`, `AbiError`/`QosClass`. This is what the crate name promises
   (ADR-0016 charter).
2. **Service↔service wire protocols** — nine hand-coded frame modules (execd,
   updated, routing, bundlemgrd, sessiond, settingsd, bundleimg, policy,
   policyd): 66 `encode_*`/`decode_*` functions, 25 opcodes, 33 status consts.
   The 3-byte magic/version guard is duplicated ~40×, the `op | 0x80` reply
   convention and the `len == 0 || len > 48` name bound are hand-repeated
   throughout, and there is **zero** shared abstraction — no macro, no trait,
   no helper. Every new service copies the "sessiond template" by hand
   (TASK-0072 says so verbatim), so the boilerplate grows linearly and each
   copy is a fresh chance for a subtle framing bug.

ADR-0038 already faced the same fork for the windowd↔gpud wire and decided:
one small Rust SSOT crate for tiny fixed little-endian frames; Cap'n Proto
stays the control-plane tool (its segment/pointer overhead loses on packed LE
frames and the IPC frame budget). That decision is landed and boot-proven —
it is the blueprint, not an open question.

## Decision

**Service wire frames live in one crate, `source/libs/nexus-wire`, and are
declared, not hand-coded.** A small codec core (`Writer`/`Reader`, the
magic/version/op header guard written once, the `op | 0x80` reply convention
written once, the RFC-0019 nonce correlation written once, strict
exact-length decoding by default) plus a `frames!` `macro_rules!` DSL generate
the per-frame `encode_*`/`decode_*` functions with the exact signatures the
nine modules expose today.

- `nexus-wire` is `no_std`, `#![forbid(unsafe_code)]`, zero dependencies,
  alloc-free (caller-provided buffers, `Option` on malformed — fail-closed).
- **Wire bytes are unchanged.** The existing golden-byte tests move verbatim
  and must pass without a single assertion edit; that is the equivalence gate.
- `nexus-abi` keeps the kernel↔userspace ABI only and re-exports the protocol
  modules at their old paths (`nexus_abi::settingsd` → `nexus_wire::settingsd`)
  as a **transitional** shim, so none of the ~51 dependent crates changes.
- The DSL's field vocabulary is closed over what the frames actually use
  (`u8/u16le/u32le/u64le`, `lit`, `magic4`, length-prefixed `str8`/`bytes8`
  with bounds, per-frame version override for policyd v1/v2/v3). Anything the
  DSL cannot express stays hand-written next to the declaration in the same
  module (e.g. bundlemgrd's CAP_MOVE'd-VMO payload-header reply) — the escape
  hatch is part of the design, not a failure of it.
- Out of scope: any Cap'n Proto migration of these frames (settled by
  ADR-0038), the RFC-0066 typed client rollout (nexus-wire is its substrate,
  separate track), and consumer import migration off the shim.

## Consequences

- **Positive**: one definition per frame; a framing bug is fixed once, in the
  engine, for all nine protocols. New service protocol = one declaration that
  reads as the frame-layout documentation. Every decoder gets the same
  fail-closed discipline plus a deterministic truncation/mutation reject
  matrix. The 4103-LOC grandfather entry disappears from the structure-gate
  baseline for real (the wire half moves out, the syscall half splits into
  `syscall/` modules).
- **Negative / accepted cost**: a macro DSL is a small language to learn;
  mitigated by keeping the field vocabulary closed and the expansion
  straight-line byte ops. The transitional re-export shim leaves imports
  pointing at `nexus_abi::<svc>` until a follow-up migrates them.
- **Follow-ups** (tracked in TASK-0296): consumer import migration + shim
  removal; consolidating the duplicate wire mirrors in
  `userspace/nexus-ipc/{logd_wire,policyd_wire}.rs` onto nexus-wire;
  `abi_filter.rs` adopting the codec core; retiring the unused `nexus-idl`
  macro crate.

## Alternatives considered

- **Plain file split, no codec** — moves the boilerplate, keeps the disease:
  every guard/bound/reply convention stays duplicated and the next service
  copies it again. Rejected as not solving the actual problem.
- **Cap'n Proto for these frames** — already measured and rejected for this
  plane by ADR-0038 (encoding overhead on tiny packed LE frames, no
  zero-copy benefit since bulk data already moves via VMO capabilities).
- **Proc-macro derive** — better hygiene than `macro_rules!` but adds a
  proc-macro crate to the build graph (the repo has none), worst-in-class
  error messages/debuggability, and is overkill for ~30 frame shapes.
- **Const field-table + generic interpreter** — cannot reproduce the existing
  heterogeneous signatures (`fn(status: u8, pid: u32) -> [u8; 9]`, borrowed
  tuple decodes) without per-frame hand wrappers, which recreates the
  boilerplate; also moves bounds checks to runtime table walks.
- **Builder/reader API only (no DSL)** — shrinks bodies ~40% but every
  signature and doc stays hand-written; the declaration never becomes the
  documentation.
