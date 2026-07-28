# ADR-0054: Map failures keep their identity across the syscall ABI — no wildcard errno arms

- Status: Accepted
- Date: 2026-07-28
- Links:
  - Tasks: `tasks/TASK-0309-gpud-framebuffer-vmo-map-ordering-hazard.md` (the investigation that forced this)
  - Related ADRs: none

## Context

The kernel's page table distinguishes five map-failure causes
(`MapError::{Unaligned, OutOfRange, InvalidFlags, PermissionDenied, Overlap}`),
and the kernel's own tests cover them. But the errno table in the trap handler
folded four of them into one:

```rust
AddressSpaceError::Mapping(MapError::PermissionDenied) => errno(EPERM),
AddressSpaceError::Mapping(_) => errno(EINVAL),          // everything else
```

So every service saw a refused mapping as a bare `InvalidArgument` — "you
passed nonsense" and "someone was here first" arrived as the same byte. During
TASK-0309 this cost a multi-boot instrumented investigation to distinguish a
VA-overrun in gpud from an overlap the kernel had already precisely diagnosed
and then erased on its way up. The information existed; the channel destroyed
it.

This is the same defect class as a `_ => "other"` arm in an error-name table —
which this investigation ALSO produced and removed twice (in gpud's diag
module and, mirrored, here). The pattern earns an ADR because the errno table
is the one surface every service depends on.

## Decision

Kernel map failures cross the ABI with their identity intact, and the errno
table never uses a wildcard arm over an error enum.

- `MapError::Overlap` → `EEXIST` (17) → `AbiError::AlreadyExists`
- `MapError::OutOfRange` → `EFAULT` (14) → `AbiError::BadAddress`
- `MapError::Unaligned` / `MapError::InvalidFlags` → `EINVAL` (22) —
  genuinely invalid arguments, named individually in the match.
- `MapError::PermissionDenied` → `EPERM` (1), unchanged.
- The `address_space_errno` match is EXHAUSTIVE with no `_` arm: a new
  `MapError` variant must fail compilation until someone assigns its errno.
- The same rule applies to userspace error-NAME tables over `AbiError`
  (gpud `diag.rs`, init `helpers.rs`, selftest `mmio.rs`): exhaustive, no
  catch-all. `AbiError::Unknown` remains the fail-closed decode for errnos
  this ABI build does not know — that is a *decoder* concern, not license for
  wildcard arms in *tables*.
- Additionally, when the kernel refuses a map as `Overlap` it logs the
  occupant (`PT-OVERLAP kind=… va=… want_pa=… occupant_pa=… occupant_flags=…`):
  a service can name *its* address and offset, but only the kernel can name
  what is already there.

Out of scope: reworking the VA-assignment architecture itself (the hand-copied
MMIO window constants and gpud's slot carve-out) — recorded in TASK-0309 as
the follow-up this decision does not solve.

## Consequences

- **Positive**: a refused map is diagnosable from the error value alone. The
  TASK-0309 class of investigation ("which of five causes was it?") ends at
  the first log line instead of after kernel instrumentation.
- **Positive**: compile-time pressure — new `MapError`/`AbiError` variants
  break every exhaustive table until they are named, which is how three
  hand-written tables (gpud, init, selftest) were found and updated in this
  change.
- **Churn accepted**: any code that matched `AbiError::InvalidArgument` to
  detect an already-mapped page now sees `AlreadyExists`. A tree-wide search
  found no such matcher; callers either propagate or name-and-log.
- **Negative**: two more errno values in the ABI contract. They are POSIX
  values used with their POSIX meaning, which bounds the surprise.
