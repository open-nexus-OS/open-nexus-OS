---
name: verify
description: Verification ladder for this repo — which gate to run for which kind of change, from fast pre-commit checks to full QEMU proof. Use before claiming any change is done.
---

# Verify (proof ladder)

Pick the smallest honest proof for the change; escalate only as far as the
change can actually break things.

| Change touches…                    | Minimum gate                                  |
|------------------------------------|-----------------------------------------------|
| docs only                          | link targets exist; no gate needed            |
| host-side Rust (userspace/, tools) | `just check` then `just test-host`            |
| a single crate                     | `cargo test -p <crate>` first, then test-host |
| OS services (source/services)      | + `just dep-gate` and `just diag-os`          |
| kernel (source/kernel)             | + `just build-kernel`; QEMU lane for behavior |
| markers / boot behavior            | + the matching QEMU lane (see boot-proof)     |
| justfile/scripts/CI                | run the touched recipe once for real          |

Full gate before finishing a work package: `just test-all`
(check → tests → e2e → miri → kernel → QEMU SMP; budget ~30 min).

## Rules

- Never claim "done" from a compile alone — run the behavior you changed.
- Dead-code deletions are only proven dead by green `just test-host` AND
  `just build-kernel` (host and OS cfgs compile different code).
- If a gate cannot run (missing tooling), say so explicitly — do not
  substitute a weaker gate silently.
- Warnings are failures here: `just diag-host` / `diag-os` / `diag-kernel`
  must stay at zero warnings.
