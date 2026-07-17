<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Build hygiene and house rules

Validation gates for OS-target builds and repository-wide house rules. Split out of the former `docs/testing/index.md`; see [README.md](README.md) for the entry point.

## Build hygiene (OS targets)

Before committing OS-related changes, run these validation gates:

| Command | Purpose |
| --- | --- |
| `just diag-os` | Check all OS services compile for `riscv64imac-unknown-none-elf` |
| `just dep-gate` | **Critical**: Fail if forbidden crates (`parking_lot`, `getrandom`) appear in OS graph |
| `just diag-host` | Check host builds compile cleanly |

### The `dep-gate` rule

OS services **must** be built with `--no-default-features --features os-lite`. Without these flags, `std`-only dependencies leak into the bare-metal build and cause cryptic errors like:

```text
error: can't find crate for `std`
  --> parking_lot_core/src/lib.rs
```

The `just dep-gate` command checks the dependency graph and **fails the build** if forbidden crates appear:

```bash
# Run before any OS commit
just dep-gate

# If it fails, check which crate pulled in the forbidden dependency:
cargo tree --target riscv64imac-unknown-none-elf -p dsoftbusd -i parking_lot
```

**See also**: `docs/standards/BUILD_STANDARDS.md` for the full feature gate convention.

## House rules

- No `unwrap`/`expect` in daemons; propagate errors with context.
- Userspace crates must keep `#![forbid(unsafe_code)]` enabled and pass Clippy's denied lints.
- No blanket `#[allow(dead_code)]` or `#[allow(unused)]`. Use the `tools/deadcode-scan.sh` guard, gate WIP APIs behind features, or add time-boxed entries to `config/deadcode.allow`.
- CI enforces architecture guards, UART markers, and formatting; keep commits green locally before pushing.
- **OS builds must pass `just dep-gate`** to ensure no `std`-only crates leak into bare-metal targets.
