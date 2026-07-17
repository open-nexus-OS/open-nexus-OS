# Security Policy

## Scope disclaimer

Open Nexus OS is a **research operating system**. It is not a production
system, ships no supported releases, and comes with **no stability or
security guarantees**. It runs against QEMU RISC-V `virt`; there is no
supported deployment on real hardware or in production environments. Please
calibrate expectations (and CVE requests) accordingly — findings are very
welcome, but they are research findings against a research target.

## Reporting a vulnerability

- Email **<jenningschaefer@googlemail.com>** with a description of the issue,
  reproduction steps (ideally against a QEMU boot or a host test), and the
  affected components.
- **Do not open public GitHub issues for exploitable findings.** Public
  issues are fine for hardening ideas, defense-in-depth suggestions, or
  non-exploitable weaknesses.
- You should receive an acknowledgment within a few days. As a research
  project there is no formal SLA for fixes, but confirmed findings are
  tracked as tasks and fixed with regression proofs.

## What is in scope

The interesting attack surface is the capability/authority model:

- **Kernel capabilities and IPC** — capability minting/transfer, endpoint and
  VMO handling, syscall guardrails (anything that lets a task exercise
  authority it was never granted).
- **`policyd` / `keystored` authority** — policy bypasses
  (deny-by-default violations, capability lookups bound to the wrong
  identity), key extraction beyond the pubkey-only export contract, audit
  trail evasion.
- **DSoftBus authentication** — session establishment, device identity
  verification, and anything that lets an unauthenticated peer join the mesh
  or impersonate a device.

Out of scope: crashes reachable only via debug/diagnostic tooling, resource
exhaustion in the QEMU harness, and issues in third-party dependencies
(report those upstream; a heads-up here is still appreciated).

## References

- Security standards and rules for this codebase:
  [docs/standards/SECURITY_STANDARDS.md](docs/standards/SECURITY_STANDARDS.md)
- Security architecture documentation: [docs/security/](docs/security/)
  (capabilities, sandboxing, identity and sessions, signing and policy)
