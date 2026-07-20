---
name: code-quality
description: The code-quality bar for this repo — non-negotiable structure (services = src/ + tests/, ≤~600 LOC, CONTEXT headers, forbid(unsafe)/no_std) plus Rust-idiom, test-authoring, and architecture patterns, each linked to its RUST_STANDARDS/docs-testing anchor and a copy-this exemplar. Use when writing, reviewing, or finishing Rust in this repo.
---

# Code quality (checklist + exemplar index)

The SSOT is `docs/standards/RUST_STANDARDS.md` + `docs/testing/`; this is the fast
checklist that routes there and points at the file to copy. For a *design/scope*
decision use the `architecture-review` skill; for *which gate to run* use `verify`.

## Non-negotiable structure (every change — mechanically gated by `just structure-gate`)

- OS crate/service = `src/` + `tests/`. Host-testable contract/reject tests live in `tests/`.
  (Legacy exceptions: `config/service-layout.allow` — shrink-only, never extend.)
- Files ≤ ~600 LOC, split by responsibility, never a god-file. Grandfathered files
  (`config/loc-baseline.txt`) may shrink but never grow; after a real split run `just structure-baseline`.
- CONTEXT header on every module (`docs/standards/DOCUMENTATION_STANDARDS.md`), kept in sync.
- `#![forbid(unsafe_code)]` in userspace crates; `no_std` (+ `alloc` only if needed) in kernel/OS/QEMU.
- Extend the existing pattern/SSOT — never fork a parallel one where an exemplar already exists.

## Rust idioms — rule → SSOT → copy this

| Rule | RUST_STANDARDS | Copy this |
|------|----------------|-----------|
| Handles are newtypes, never raw `usize`/`u64` | §6 (newtypes prevent handle confusion) | `source/kernel/neuron/src/types.rs` (`Pid`), `source/services/windowd/src/ids.rs` |
| `#[must_use]` on resource/security types | §8 (security errors must_use) | `source/init/nexus-init/src/route_table.rs` (`CapSlot`, custom message) |
| `Send`/`Sync` justified; CPU-local via discipline | §4 / §7 (concurrency) | `source/kernel/neuron/src/sync/percpu.rs` (`PerCpu` + SAFETY) |
| No `unwrap`/`expect` on untrusted input; propagate with context | §5 (error handling) | daemons return typed `Result`, e.g. `source/services/queryd` |
| `unsafe` only where necessary, small, with SAFETY comment | §4 (unsafe policy) | `source/kernel/neuron/src/sync/percpu.rs` |

## Tests you must write — change → test → copy this

| New/changed | Write | Copy this |
|-------------|-------|-----------|
| Service wire/opcode | contract test: opcode round-trip + malformed reject + accept-boundary | `source/services/samgrd/tests/malformed_contract.rs`, `source/services/queryd/tests/loopback.rs` |
| Security-relevant surface | `test_reject_*` negatives, one typed error each | `source/libs/nexus-abi/tests/abi_filter_reject.rs` |
| Cross-service behavior | hop marker per stage along the chain | `tools/nx/chains/markers.txt` + `tools/nx/src/chain/tests.rs` (`contract_covers_markers`) |
| Boot proof | pure-observer selftest (assert on UART markers, don't drive) | `source/apps/selftest-client/README.md` + `proof-manifest/markers/*.toml` |

- Marker discipline: changing a marker string = update the emitter **and** `tools/nx/chains/markers.txt`
  (chain) **or** `source/apps/selftest-client/proof-manifest/markers/<phase>.toml` (selftest) **and** docs,
  in the same commit. Which gate proves it → `verify` skill / `boot-proof` skill.

## Architecture defaults

- **Declarative over hardcoded:** add a service/route = one row in the SSOT
  (`source/libs/nexus-sdk-routes/src/lib.rs`, `source/init/nexus-init/src/service_topology.rs`), not a
  bespoke `match` arm. The host-testable topology cross-validates it.
- **Factory capabilities over hardcoded slots:** mint per-launch via the factory (`@mint-pair` in
  `source/init/nexus-init/src/bootstrap/responder.rs` → mint→grant→close); never hand a shared/persistent
  slot to a child.
- **Modern MMIO:** access through the `Bus` trait (`source/libs/nexus-hal/src/lib.rs`) /
  `VirtioMmio<B: Bus>` (`source/libs/nexus-virtio`), mockable on host — never raw pointer poking. MMIO maps
  USER|RW, never executable (`nexus-abi::mmio_map`). Device server = MMIO + command-encoding + reset
  (`source/libs/nexus-driverkit`).

## Rules

- No `unsafe` in userspace; kernel `unsafe` needs a small block + SAFETY comment.
- No god-file: ≤ ~600 LOC, `src/` + `tests/`, CONTEXT header — always.
- No parallel pattern where an SSOT row or exemplar exists — extend it.
- A security-relevant change without a `test_reject_*` is unfinished.
- A marker change without moving its contract (markers.txt / proof-manifest + docs) is broken.
- New service / API / syscall / wire format → run the `architecture-review` skill first.
