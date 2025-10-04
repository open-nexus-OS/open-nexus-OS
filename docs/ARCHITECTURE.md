# Userspace vs. services layering

The Open Nexus workspace enforces a host-first development workflow. Domain
logic lives in `userspace/<crate>` libraries that expose safe, testable APIs
behind mutually exclusive `backend-host` and `backend-os` features. Each
`userspace` crate must compile with exactly one backend selected and forbids all
unsafe code.

Daemons under `source/services/<name>d` are thin adapters. They register with
`samgr`, expose IDL bindings, and forward requests into the corresponding
userspace crate compiled with the `backend-os` feature.

The `tools/arch-check` utility fails CI when a userspace crate depends on the
kernel, HAL, samgrd, nexus-abi, or any crate under `source/services/`. This
preserves the separation between host-tested logic and system wiring.

## Control-plane IDL strategy

Cap'n Proto schemas live exclusively in userspace under `tools/nexus-idl` with
generated Rust emitted by `userspace/nexus-idl-runtime`. Kernel components never
parse Cap'n Proto payloads; they only shuttle handles or VMOs referenced in the
metadata. Service daemons link `nexus-idl-runtime` to translate Cap'n Proto
messages into safe userspace library calls while bulk payloads continue to move
out-of-band via VMOs and `map()`.
