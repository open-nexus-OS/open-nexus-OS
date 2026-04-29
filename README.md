# open-nexus-os

[![Software DOI](https://img.shields.io/badge/DOI-10.5281%2Fzenodo.18934993-blue)](https://doi.org/10.5281/zenodo.18934993)

Support ongoing development:

[![Sponsor open-nexus-OS](https://img.shields.io/badge/Sponsor-GitHub-ea4aaa?logo=githubsponsors&logoColor=white)](https://github.com/sponsors/open-nexus-OS)

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

The repo intentionally has **two complementary entrypoint spurs**, neither of which calls into the other:

- `just …` — developer ergonomics for host-first iteration and granular QEMU profile matrix. Lazy-build (every recipe rebuilds what it needs).
- `make …` — self-contained "container CI / QEMU-last" pipeline. Strict build → test → run discipline.

Pick whichever fits the flow you're in.

### `just` spur (dev / per-task iteration)

- **Host tests**: `just test-host`, `just test-e2e`
- **Miri**: `just miri-strict`, `just miri-fs`
- **Architecture guard**: `just arch-check`
- **QEMU smoke**: `RUN_UNTIL_MARKER=1 just test-os` (wraps `scripts/qemu-test.sh`)
- **Aggregate release gate**: `just test-all` (fmt + clippy + deny + host + e2e + miri + arch + kernel + SMP CI)

### Make spur (build → test → run, no `just` dependency)

The `Makefile` is the self-contained "build once, then test/run against the artifacts" path. It is intentionally **not** a thin shim over `just`; it owns its own build step.

```sh
make build           # compile only: host workspace + host test binaries + cross OS services + neuron-boot (with EMBED_INIT_ELF)
make test            # host-first nextest run + SMP=2 then SMP=1 QEMU ladder against the `make build` artifacts
make run             # one QEMU smoke (smart smp/full profile pick) against the `make build` artifacts
```

| Target | Owns the build? | Runs QEMU? | Notes |
|---|---|---|---|
| `make build` | yes (single source of truth for compilation) | no | `MODE=container` (default, podman) or `MODE=host` (direct cargo). Builds **and** pre-compiles host test binaries. |
| `make test` | no — requires `make build` to have run | yes — SMP=2 (`--profile=smp`) then SMP=1 (`--profile=full`) | Host tests run via `cargo nextest` (instant if `make build` ran), then the QEMU ladder. |
| `make run` | no — requires `make build` to have run | yes — single boot, profile auto-picked from `SMP` env (or `PROFILE=…`) | For fast local iteration use `RUN_UNTIL_MARKER=1 make run` (default). |

`make test` and `make run` set `NEXUS_SKIP_BUILD=1` when invoking `scripts/qemu-test.sh` / `scripts/run-qemu-rv64.sh`. The script then skips its per-component `cargo build` calls and **fails fast** with a clear `[error] NEXUS_SKIP_BUILD=1 but … artifact is missing — run 'make build' first` message if any artifact is absent. This makes "I forgot `make build`" a loud one-line error instead of a silent 30-second rebuild.

If you want the historical eager-rebuild behavior in a one-shot run, use `NEXUS_SKIP_BUILD=0 make run` or chain `make build run`.

`make verify` was retired; the canonical aggregate gate is `just test-all`.

## Current engineering focus

- Kernel baseline behavior is stabilized and continuously regression-tested with deterministic QEMU marker gates.
- Userspace syscall guardrail hardening (`TASK-0019` / `RFC-0032`) is closed as done with authenticated profile distribution and fail-closed behavior proofs.
- `TASK-0020` (DSoftBus streams v2) is closed as `Done` with host + single-VM + 2-VM + perf/soak evidence.
- `TASK-0021` (DSoftBus QUIC v1 host-first scaffold) is closed as `Done` with real host QUIC transport proof, QUIC+mux payload smoke proof, strict fail-closed mode semantics, and deterministic OS fallback markers.
- `TASK-0023` is closed as a gated-contract slice (`Done`) with blocked/no-go unlock outcome; current sequential queue head is `TASK-0024` unless explicitly resequenced.
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
