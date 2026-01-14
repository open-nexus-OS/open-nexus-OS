# System map & boundaries (onboarding)

This page gives a **high-level mental model** of the Open Nexus OS tree and the **hard boundaries** that prevent architecture drift.

If you only read one thing before changing code, read:

- `tasks/README.md` (execution truth + proof rules)
- `docs/standards/DOCUMENTATION_STANDARDS.md` (headers, ownership, drift handling)
- `docs/adr/0001-runtime-roles-and-boundaries.md` (single sources of truth)

## Big picture (what lives where)

Open Nexus OS is developed **host-first, QEMU-last**:

- Most domain logic lives in **host-testable libraries**.
- OS “services” are thin adapters that compile for bare-metal and forward into those libraries.
- QEMU runs are **bounded** and validated via **deterministic UART markers**.

The repo is structured around that:

- `userspace/`
  - **Domain libraries** and SDK-style crates.
  - Must stay safe and testable (host-first); commonly `#![forbid(unsafe_code)]`.
  - Use `nexus_env="host"` vs `nexus_env="os"` to select backends.
- `source/services/`
  - **Daemon processes** (`*d`) that provide OS services via IPC/IDL.
  - Intentionally thin: wiring + validation + error propagation; business logic lives in `userspace/`.
- `source/kernel/neuron/` + `source/kernel/neuron-boot/`
  - The NEURON kernel runtime + boot wrapper.
  - Kernel stays minimal: no policy, no crypto, no IDL parsing.
- `tests/`
  - Host integration and E2E harnesses (fast, deterministic, no QEMU).
- `docs/`
  - Architecture notes, ADRs/RFCs, standards, testing methodology.
- `tasks/`
  - The **execution truth**: DoD, proofs, allowed touched paths.

## Runtime roles (single sources of truth)

The project explicitly avoids duplicated "competing implementations".
Canonical roles (see ADR‑0001):

- **Init / orchestrator**: `source/init/nexus-init` (host backend + os-lite backend)
- **Spawner**: `source/services/execd`
- **Loader library**: `userspace/nexus-loader`
- **Kernel loader**: thin ABI bridge only (`source/kernel/neuron/src/user_loader.rs`)
- **Fixtures / test payloads**: `userspace/exec-payloads`, demo payload crates
- **Observability authority**: `source/services/logd` (bounded RAM journal, crash reports)
- **Logging facade**: `source/libs/nexus-log` (unified API for services)

## Boundaries that must not be crossed (anti-drift)

These boundaries are what keep the “host-first” strategy real:

- **No userspace → kernel/service dependencies**
  - Userspace crates must not depend on kernel crates, HAL, or service daemons.
  - Enforced mechanically via `tools/arch-check`.
- **No “fake green”**
  - Don’t add `*: ready` / `SELFTEST: … ok` markers unless behavior really happened.
  - Deterministic marker contract is centralized in `scripts/qemu-test.sh`.
- **Contracts belong to the right doc**
  - **Tasks** own: scope + DoD + proof commands/markers.
  - **RFCs** own: interfaces/contracts/invariants.
  - **ADRs** own: narrow decisions and rationale.

## Where to look when changing something

- **You changed behavior**: update the task proof section + the relevant RFC/ADR (and add a drift note if needed).
- **You changed marker semantics**: update `scripts/qemu-test.sh` and `docs/testing/index.md` (don’t duplicate marker lists elsewhere).
- **You added a new subsystem**: add a task first, then an RFC/ADR if it introduces a new contract/boundary.
