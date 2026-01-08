# Build Standards: Feature Gates & Dependency Hygiene

**Created**: 2026-01-07  
**Owner**: @runtime  
**RFC**: RFC-0009 (no_std Dependency Hygiene v1)

## Overview

This document defines the build standards for Open Nexus OS to prevent dependency leakage between host and OS targets. **Failure to follow these rules causes hard-to-debug build failures on bare-metal targets.**

---

## The Golden Rule

> **OS services MUST be built with `--no-default-features --features os-lite`**

This ensures that `std`-only dependencies (like `parking_lot`, `getrandom`) do not leak into bare-metal builds.

---

## Feature Gate Convention

### Standard Feature Structure

Every crate that supports both host and OS targets MUST define these features in `Cargo.toml`:

```toml
[features]
default = ["std"]           # Host builds get std by default
std = []                    # Enables std-dependent code
os-lite = []                # Enables no_std-compatible code path
```

### Code Organization

```rust
// In lib.rs or main.rs

// Conditionally enable no_std for OS builds
#![cfg_attr(
    all(feature = "os-lite", not(feature = "std"), nexus_env = "os"),
    no_std
)]

// REQUIRED: Declare alloc crate in no_std modules that use Vec, BTreeMap, etc.
#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(feature = "std")]
mod host_impl;              // Uses std, parking_lot, getrandom, etc.

#[cfg(feature = "os-lite")]
mod os_impl;                // Uses no_std + alloc only

#[cfg(all(not(feature = "std"), not(feature = "os-lite")))]
compile_error!("Either 'std' or 'os-lite' feature must be enabled");
```

### The `extern crate alloc` Rule

**Critical**: In `no_std` crates that use `alloc` types (`Vec`, `String`, `BTreeMap`, etc.), you MUST declare:

```rust
extern crate alloc;

use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::String;
```

This is different from `std` builds where `alloc` is re-exported automatically. Forgetting this causes:
```
error[E0433]: failed to resolve: use of undeclared crate or module `alloc`
```

---

## Forbidden Crates (OS Target)

These crates MUST NOT appear in the dependency graph for `riscv64imac-unknown-none-elf`:

| Crate | Reason | Alternative |
|-------|--------|-------------|
| `parking_lot` | Requires OS threads | `spin` or `critical-section` |
| `parking_lot_core` | Transitively pulls in std | (same) |
| `getrandom` | Requires OS entropy | Deterministic seeds or hardware RNG |
| `std` | Bare-metal has no std | `core` + `alloc` |

### Validation Command

```bash
just dep-gate
```

This command checks the OS dependency graph and **fails if forbidden crates appear**.

---

## Build System Rules

### Nightly Toolchain Requirement

**RISC-V bare-metal builds require the nightly toolchain** for certain unstable features. Always use:

```bash
cargo +nightly-2025-01-15 build --target riscv64imac-unknown-none-elf ...
```

The pinned nightly version is defined in:
- `Makefile`: `NIGHTLY ?= nightly-2025-01-15`
- `justfile`: `toolchain := "nightly-2025-01-15"`

### Makefile

When building OS services in `Makefile`:

```makefile
# ✅ CORRECT: Build OS services with os-lite AND nightly toolchain
cargo +$(NIGHTLY) build --target riscv64imac-unknown-none-elf \
    --no-default-features --features os-lite \
    -p my-service

# ❌ WRONG: Missing feature flags (pulls in std dependencies!)
cargo build --target riscv64imac-unknown-none-elf \
    -p my-service

# ❌ WRONG: Missing nightly toolchain
cargo build --target riscv64imac-unknown-none-elf \
    --no-default-features --features os-lite \
    -p my-service
```

### Library-Only Crates

Some crates have a binary (`[[bin]]`) that only makes sense on host. For OS builds, build only the library:

```makefile
# ✅ CORRECT: Build only the library for crates with host-only binaries
cargo +$(NIGHTLY) build -p nexus-init --lib \
    --target riscv64imac-unknown-none-elf \
    --no-default-features --features os-lite

# ❌ WRONG: Building the binary fails on no_std (missing main, panic_handler)
cargo +$(NIGHTLY) build -p nexus-init \
    --target riscv64imac-unknown-none-elf \
    --no-default-features --features os-lite
```

This is necessary because:
- `no_std` binaries need `#![no_main]` and a panic handler
- The actual OS binary is `init-lite`, not `nexus-init`

### justfile

The `justfile` MUST be consistent with `Makefile`:

```just
# diag-os target must use same flags as Makefile
diag-os:
    cargo check --target riscv64imac-unknown-none-elf \
        --no-default-features --features os-lite \
        -p my-service
```

### scripts/run-qemu-*.sh

QEMU scripts MUST reference binaries built with correct flags:

```bash
# Binaries should come from:
# target/riscv64imac-unknown-none-elf/release/my-service
# NOT from default build without --no-default-features
```

---

## Adding a New OS Service

### Checklist

1. [ ] `Cargo.toml` has `default = ["std"]` and `os-lite` features
2. [ ] Code compiles with `--no-default-features --features os-lite`
3. [ ] No forbidden crates in dependency graph (`just dep-gate` passes)
4. [ ] `Makefile` builds service with `--no-default-features --features os-lite`
5. [ ] `justfile` `diag-os` target includes the service
6. [ ] Service excluded from OS build if it cannot support `os-lite`

### Template Cargo.toml

```toml
[package]
name = "my-service"
version = "0.1.0"
edition = "2021"

[features]
default = ["std"]
std = ["dep:parking_lot"]  # Host-only dependencies gated here
os-lite = []               # No additional deps for OS

[dependencies]
# Always available (no_std compatible)
log = "0.4"

# Host-only (gated behind std feature)
parking_lot = { version = "0.12", optional = true }

[target.'cfg(all(target_os = "none", target_arch = "riscv64"))'.dependencies]
# OS-only dependencies
spin = "0.9"
```

### Feature Propagation for Multi-Crate Dependencies

When a crate depends on other crates that have their own `std`/`os-lite` features, you MUST:

1. **Disable default features** with `default-features = false`
2. **Explicitly enable the required feature** in your feature definition

```toml
[features]
default = ["std"]
std = [
    "my-dep",
    "my-dep/default",      # Explicitly enable default for host
]
os-lite = [
    "my-dep",
    "my-dep/os-lite",      # Propagate os-lite to dependency
]

[dependencies]
# CRITICAL: default-features = false allows feature control
my-dep = { path = "../my-dep", optional = true, default-features = false }
```

**Without `default-features = false`**, Cargo builds the dependency with its default features, causing `std` dependencies to leak into `os-lite` builds!

Example failure mode:
```
# nexus-init depends on samgrd
samgrd = { path = "../../services/samgrd", optional = true }  # ❌ WRONG

# samgrd has default = ["std", "idl-capnp"]
# This pulls in serde, capnp, etc. even when nexus-init uses os-lite!

# ✅ CORRECT:
samgrd = { path = "../../services/samgrd", optional = true, default-features = false }
```

---

## Diagnostics

### just diag-os

Checks all OS services compile for bare-metal:

```bash
just diag-os
```

**Must pass before any OS commit.**

### just dep-gate

Checks for forbidden crates in OS dependency graph:

```bash
just dep-gate
```

**Fails if `parking_lot`, `parking_lot_core`, or `getrandom` appear.**

---

## Common Mistakes

### Mistake 1: Building without feature flags

```bash
# ❌ WRONG
cargo build --target riscv64imac-unknown-none-elf -p dsoftbusd

# ✅ CORRECT
cargo build --target riscv64imac-unknown-none-elf \
    --no-default-features --features os-lite -p dsoftbusd
```

### Mistake 2: Transitive dependencies

A crate may be `no_std` but pull in `std` dependencies transitively:

```toml
# ❌ WRONG: getrandom pulls in std
rand = "0.8"  # default features include getrandom

# ✅ CORRECT: disable default features
rand = { version = "0.8", default-features = false }
```

### Mistake 3: Forgetting to update Makefile

If you add a new OS service, you MUST:

1. Add it to `Makefile` OS build targets
2. Use `--no-default-features --features os-lite`
3. Run `just dep-gate` to verify

### Mistake 4: cfg(nexus_env) vs cfg(feature)

```rust
// ❌ WRONG: nexus_env is for conditional compilation, not feature gating
#[cfg(nexus_env = "os")]
use parking_lot::Mutex;  // parking_lot won't compile on OS!

// ✅ CORRECT: use feature gates for dependency selection
#[cfg(feature = "std")]
use parking_lot::Mutex;

#[cfg(feature = "os-lite")]
use spin::Mutex;
```

### Mistake 5: Missing `extern crate alloc`

```rust
// ❌ WRONG: alloc is not automatically available in no_std
use alloc::vec::Vec;  // error: use of undeclared crate or module `alloc`

// ✅ CORRECT: explicitly declare alloc crate
extern crate alloc;
use alloc::vec::Vec;
```

### Mistake 6: Unused variables in conditional code

```rust
// ❌ WRONG: `s` is unused when cfg doesn't match
fn debug_print(s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64"))]
    uart_write(s);  // s is unused on host builds
}

// ✅ CORRECT: prefix with underscore to indicate intentional non-use
fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64"))]
    uart_write(_s);
}
```

### Mistake 7: Feature-gated modules without guard

```rust
// ❌ WRONG: full_impl uses capnp but is compiled without idl-capnp feature
mod full_impl;  // Contains: use capnp::...

// ✅ CORRECT: guard the module with the required feature
#[cfg(feature = "idl-capnp")]
mod full_impl;
```

---

## History

### 2026-01-07: Feature Propagation Fix

**Problem**: `make build MODE=host` failed with `serde_core` errors on RISC-V target.

**Root Causes**:
1. `Makefile` was building OS services on x86_64-host without `--target riscv64imac-unknown-none-elf`
2. `nexus-init/Cargo.toml` referenced services without `default-features = false`
3. Missing `extern crate alloc;` in `vfsd` and `packagefsd` os_lite modules
4. `keystored/lib.rs` included `full_impl` module without `idl-capnp` feature guard
5. `nexus-init` binary was built for RISC-V but lacks `#![no_main]` and panic handler

**Solution**:
1. Added `--target riscv64imac-unknown-none-elf` to Makefile OS service builds
2. Added `+$(NIGHTLY)` toolchain selector for RISC-V builds
3. Added `default-features = false` to all service dependencies in `nexus-init`
4. Explicit feature propagation: `"samgrd/os-lite"` in feature definitions
5. Added `--lib` flag for `nexus-init` (binary is host-only)
6. Added `extern crate alloc;` to os_lite modules
7. Added `#[cfg(feature = "idl-capnp")]` guard to `keystored/lib.rs`

### 2026-01-07: RFC-0009 Implementation

**Problem**: OS services built without `--no-default-features --features os-lite` caused `parking_lot` and `getrandom` to leak into bare-metal builds.

**Root Cause**: `Makefile` line 39 and 53 were building OS services without proper feature flags.

**Solution**:
1. Fixed `Makefile` to use `--no-default-features --features os-lite`
2. Excluded services without `os-lite` feature from OS build
3. Added `just dep-gate` command to enforce dependency hygiene
4. Created this document to prevent future occurrences

**RFC**: `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md`

---

## Related Documents

- `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md` — Full RFC with rationale
- `docs/architecture/networking-authority.md` — Networking feature gates
- `.cargo/config.toml` — Cargo configuration (check-cfg, default target)
- `rust-toolchain.toml` — Toolchain pinning
