# open-nexus-os

Open Nexus OS is a research microkernel targeting QEMU RISC‑V `virt` hardware with an OpenHarmony‑inspired userland.
The repo is run **host-first, QEMU-last**: most logic lives in host-testable userspace crates, while QEMU is used for bounded end-to-end smoke proofs.

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

## Day-to-day development

Use the `justfile` for the primary developer workflow (host-first tests and diagnostics; QEMU-last smoke):

- **Host tests**: `just test-host`, `just test-e2e`
- **Miri**: `just miri-strict`, `just miri-fs`
- **Architecture guard**: `just arch-check`
- **QEMU smoke**: `RUN_UNTIL_MARKER=1 just test-os` (wraps `scripts/qemu-test.sh`)

`make` remains the orchestration entrypoint for setup/build/run (and is used by CI for build verification).

## How we work (authority model)

- **Tasks (`tasks/TASK-*.md`) are the execution truth**: scope, stop conditions (DoD), and proof commands/markers.
- **RFCs (`docs/rfcs/RFC-*.md`) are design seeds / contracts**: interfaces, invariants, versioning; they must link to tasks for implementation + evidence.
- **ADRs (`docs/adr/*.md`) are narrow decision records**: one decision + rationale.

Start here:

- `tasks/README.md` (workflow + anti-drift rules)
- `docs/rfcs/README.md` (RFC process + authority model)

## Updates (OTA v1.0)

The repository includes a **userspace-only A/B update skeleton** (non-persistent) that
proves stage → switch → health → rollback behavior via deterministic QEMU markers.
See `docs/updates/ab-skeleton-v1.md` and `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`.

## Documentation and standards

- **Project overview**: `docs/overview.md`
- **Architecture index**: `docs/architecture/README.md`
- **Testing methodology (host-first, QEMU-last)**: `docs/testing/index.md`
- **Observability (logd journal + crash reports)**: `docs/observability/logging.md`
- **Docs/code standards (headers, ownership, drift rules)**: `docs/standards/DOCUMENTATION_STANDARDS.md`

## Continuous Integration

CI is checked in under `.github/workflows/`:

- **`ci.yml`**: fmt + clippy + host tests + remote E2E + Miri + deadcode scan, plus a bounded QEMU run via `scripts/qemu-test.sh`.
- **`build.yml`**: build/verification workflow (includes `make initial-setup` and `make build MODE=host`; optional OS smoke job).
