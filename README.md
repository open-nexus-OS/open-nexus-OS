# open-nexus-os

[![Software DOI](https://img.shields.io/badge/DOI-10.5281%2Fzenodo.18934993-blue)](https://doi.org/10.5281/zenodo.18934993)

Open Nexus OS is a research microkernel targeting QEMU RISC‑V `virt` hardware with an OpenHarmony‑inspired userland.
The repo is run **host-first, QEMU-last**: most logic lives in host-testable userspace crates, while QEMU is used for bounded end-to-end smoke proofs.

## Citation

If you use Open Nexus OS in research or reference its architecture, please cite the software record and the related architecture series papers.

- Software DOI: [10.5281/zenodo.18934993](https://doi.org/10.5281/zenodo.18934993)
- Part I, Type-Driven Deterministic Construction of a Capability Microkernel: [10.5281/zenodo.18935402](https://doi.org/10.5281/zenodo.18935402)
- Part II, Bounded Control-Plane IPC and Explicit Bulk Data Paths: [10.5281/zenodo.18935755](https://doi.org/10.5281/zenodo.18935755)
- Part III, Service Planes and a Capability-Governed Mesh: [10.5281/zenodo.18938789](https://doi.org/10.5281/zenodo.18938789)
- Part IV, Userspace Device Services Substrate: [10.5281/zenodo.18939217](https://doi.org/10.5281/zenodo.18939217)
- Contract-Governed LLM Development: [10.5281/zenodo.18941284](https://doi.org/10.5281/zenodo.18941284)

## Quickstart (new machine)

```sh
git clone <repo>
cd open-nexus-OS
make initial-setup
```

Then try a build and a boot:

```sh
make build
make run
```

### QEMU (virtio-mmio modern for persistence)

StateFS persistence requires **modern virtio-mmio** semantics for virtio-blk.
The canonical QEMU harness (`scripts/run-qemu-rv64.sh` via `just test-os`) enforces this by default using:
`-global virtio-mmio.force-legacy=off`.

If you need a standalone QEMU build with force-modern defaults (e.g. an external harness that cannot set QEMU globals),
you can build the local modern-QEMU variant:

```sh
./tools/qemu/build-modern.sh
export PATH="$PWD/tools/qemu-src/build:$PATH"
```

## Day-to-day development

Use the `justfile` for the primary developer workflow (host-first tests and diagnostics; QEMU-last smoke):

- **Host tests**: `just test-host`, `just test-e2e`
- **Miri**: `just miri-strict`, `just miri-fs`
- **Architecture guard**: `just arch-check`
- **QEMU smoke**: `RUN_UNTIL_MARKER=1 just test-os` (wraps `scripts/qemu-test.sh`)

`make` remains the orchestration entrypoint for setup/build/run (and is used by CI for build verification).

Test entrypoints:

- `make test`: quick host-first workspace tests (kernel crates excluded).
- `make verify`: full verification gate (delegates to `just` diagnostics/tests + QEMU smoke; optional SMP dual-mode with `REQUIRE_SMP_VERIFY=1`).

## Current engineering focus

- Kernel baseline behavior is stabilized and continuously regression-tested with deterministic QEMU marker gates.
- Userspace syscall guardrail hardening (`TASK-0019` / `RFC-0032`) is closed as done with authenticated profile distribution and fail-closed behavior proofs.
- `TASK-0020` (DSoftBus streams v2) is closed as `Done` with host + single-VM + 2-VM + perf/soak evidence.
- `TASK-0021` (DSoftBus QUIC v1 host-first scaffold) is closed as `Done` with real host QUIC transport proof, QUIC+mux payload smoke proof, strict fail-closed mode semantics, and deterministic OS fallback markers.
- Next sequential queue head is `TASK-0022`.
- Canonical security proof ladder:
  - `cargo test -p nexus-abi -- reject --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## How we work (authority model)

- **Tasks (`tasks/TASK-*.md`) are the execution truth**: scope, stop conditions (DoD), and proof commands/markers.
- **RFCs (`docs/rfcs/RFC-*.md`) are design seeds / contracts**: interfaces, invariants, versioning; they must link to tasks for implementation + evidence.
- **ADRs (`docs/adr/*.md`) are narrow decision records**: one decision + rationale.

Start here:

- `tasks/README.md` (workflow + anti-drift rules)
- `docs/rfcs/README.md` (RFC process + authority model)

## Git workflow (quick reference)

- Run `make initial-setup` to install hooks.
- Use `just diag-host` and `just dep-gate` before OS commits.
- Keep commits scoped to a single task intent.

## Updates (OTA v1.0)

The repository includes a **userspace-only A/B update skeleton** (non-persistent) that
proves stage → switch → health → rollback behavior via deterministic QEMU markers.
See `docs/updates/ab-skeleton-v1.md` and `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`.

## Security (Policy Authority + Audit)

The OS uses a capability-based policy model with deny-by-default semantics:

- **Policy Engine (`nexus-sel`)**: Service-id based capability lookups
- **Audit Trail**: All allow/deny decisions logged via `logd`
- **Channel-bound Identity**: Policy binds to kernel-provided `sender_service_id`
- **Policy-gated Operations**: Sensitive operations (signing, exec, bundle install) require explicit capabilities

See `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` and `docs/architecture/11-policyd-and-policy-flow.md`.

## Security (Device Identity Keys)

OS builds cannot rely on `getrandom` for entropy. Device identity keys are generated on OS/QEMU via a bounded, policy-gated path:

- virtio-rng MMIO → `rngd` (entropy authority) → `keystored` (device keygen + pubkey-only export)

See `docs/rfcs/RFC-0016-device-identity-keys-v1.md` and `docs/security/identity-and-sessions.md`.

## Documentation and standards

- **Project overview**: `docs/overview.md`
- **Architecture index**: `docs/architecture/README.md`
- **Testing methodology (host-first, QEMU-last)**: `docs/testing/index.md`
- **Observability (logd journal + crash reports)**: `docs/observability/logging.md`
- **Security (policy + audit)**: `docs/security/signing-and-policy.md`
- **Docs/code standards (headers, ownership, drift rules)**: `docs/standards/DOCUMENTATION_STANDARDS.md`

## Continuous Integration

CI is checked in under `.github/workflows/`:

- **`ci.yml`**: fmt + clippy + host tests + remote E2E + Miri + deadcode scan, plus a bounded QEMU run via `scripts/qemu-test.sh`.
- **`build.yml`**: build/verification workflow (includes `make initial-setup` and `make build MODE=host`; optional OS smoke job).
