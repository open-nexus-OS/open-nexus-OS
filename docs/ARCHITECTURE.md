# Userspace vs. services layering

The Open Nexus workspace enforces a host-first development workflow. Domain
logic lives in `userspace/<crate>` libraries that expose safe, testable APIs
behind mutually exclusive `nexus_env="host"` and `nexus_env="os"` configurations.
Each `userspace` crate must compile with exactly one environment selected via
`RUSTFLAGS='--cfg nexus_env="..."'` and forbids all unsafe code.

Daemons under `source/services/<name>d` are thin adapters. They register with
`samgr`, expose IDL bindings, and forward requests into the corresponding
userspace crate compiled with `nexus_env="os"`.

The `tools/arch-check` utility fails CI when a userspace crate depends on the
kernel, HAL, samgrd, nexus-abi, or any crate under `source/services/`. This
preserves the separation between host-tested logic and system wiring.

### Kernel bring-up guardrails (current increment)

- Strict stage policy: early boot uses raw UART only, no heavy formatting/alloc before selftests.
- Minimal `spawn`/`cap_transfer` implemented to exercise task control and rights derivation.
- Selftests run on a private stack with canaries and masked timer IRQs; UART markers allow deterministic CI exit.
- Optional trap symbolization (`trap_symbols`) prints nearest function for `sepc` without runtime cost when disabled.

## Control-plane IDL strategy

Cap'n Proto schemas live exclusively in userspace under `tools/nexus-idl` with
generated Rust emitted by `userspace/nexus-idl-runtime`. Kernel components never
parse Cap'n Proto payloads; they only shuttle handles or VMOs referenced in the
metadata. Service daemons link `nexus-idl-runtime` to translate Cap'n Proto
messages into safe userspace library calls while bulk payloads continue to move
out-of-band via VMOs and `map()`.
