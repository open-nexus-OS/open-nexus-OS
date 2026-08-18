<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Testing methodology

Open Nexus OS follows a **host-first, OS-last** strategy. Most logic is exercised with fast host tools, leaving QEMU for end-to-end smoke coverage only. This README is the entry point: it holds the philosophy, the workflow checklist, and a document map routing to the focused testing docs (formerly a single `index.md`).

**Related RFCs:**
- **RFC-0013**: Boot gates v1 — readiness contract + spawn failure reasons (Complete)
- **RFC-0014**: Testing contracts v1 — host-first service contract tests + phased QEMU smoke (Complete)
- **RFC-0015**: Policy Authority & Audit Baseline v1 — policy engine + audit trail (Complete)
- **RFC-0017**: Device MMIO access model v1 — capability-gated MMIO mapping + init/policy distribution (Done)
- **RFC-0019**: IPC request/reply correlation v1 — nonce correlation + deterministic QEMU virtio-mmio policy (Complete)
- **RFC-0031**: Crashdumps v1 — deterministic in-process minidumps + host symbolization (Complete)
- **RFC-0038**: Selftest-client production-grade deterministic test architecture refactor + manifest/evidence/replay v1 — proof-manifest SSOT, signed evidence bundles, replay/bisect tooling (Done; one environmental closure step remaining — external CI-runner replay artifact for P6-05, see `docs/testing/replay-and-bisect.md` §7-§11)
- **RFC-0046**: UI v1a host CPU renderer + deterministic snapshots — BGRA8888 host renderer, deterministic goldens, reject tests, and fake-marker prohibition (Done; `TASK-0054` Done)
- **RFC-0047**: UI v1b windowd surface/layer/present — headless `windowd` compositor contract, bounded surface/VMO/layer rejects, generated Cap'n Proto roundtrips, and honest QEMU markers (Done; `TASK-0055` Done)
- **RFC-0048**: UI v1c visible QEMU scanout bootstrap — fixed-mode QEMU `ramfb`, capability-gated `fw_cfg` setup, visible marker ladder, and harness-profile proof (Done; `TASK-0055B` Done)

## Philosophy

- Prioritise fast feedback by writing unit, property, and contract tests in userspace crates first.
- Keep kernel selftests focused on syscall, IPC, and VMO surface validation; they should advertise success through UART markers that CI can detect.
- Reserve QEMU for smoke and integration validation. Runs are bounded by a timeout and produce trimmed logs to avoid multi-gigabyte artefacts.
- No fake-success markers: `*: ready` / `SELFTEST: * ok` strings may only be used as proof when the associated behavior is asserted by deterministic tests or harness checks.

## Testing layers at a glance

| Layer | Scope | Typical command | Details |
| --- | --- | --- | --- |
| Kernel selftests (`source/kernel/neuron`) | Traps, scheduler, IPC router, spawn; success via UART markers | `RUN_UNTIL_MARKER=1 just test-os` | [layers.md](layers.md) |
| Userspace libraries (`userspace/`) | Host-first unit/property/contract tests, golden vectors, Miri | `cargo test --workspace` | [layers.md](layers.md) |
| Services and daemons (`source/services/*d`) | Thin IPC adapters; IDL round-trip + contract tests | `cargo test -p <svc>` | [layers.md](layers.md) |
| Host E2E suites (`tests/`) | In-process loopback E2E: `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`, `e2e_policy` | `just test-e2e` | [e2e.md](e2e.md), [e2e-coverage-matrix.md](e2e-coverage-matrix.md) |
| QEMU smoke (`scripts/qemu-test.sh`) | Kernel selftests + service readiness marker ladder | `RUN_UNTIL_MARKER=1 just test-os` | [os-markers.md](os-markers.md) |
| QEMU 2-VM opt-in (`tools/os2vm.sh`) | Cross-VM DSoftBus discovery, Noise sessions, remote proxy | `just ci-os-os2vm` | [layers.md](layers.md), [network-distributed-debugging.md](network-distributed-debugging.md) |

The full layer reference — including the end-to-end coverage table and the per-TASK requirement matrices (TASK-0020 … TASK-0057) — lives in [layers.md](layers.md).

> **`just ci-network` is currently RED — known, tracked, not a regression you introduced.**
> Measured 2026-08-05: `ci-os-dhcp` green, `ci-os-quic` and `ci-os-os2vm` red.
> Cross-VM DSoftBus discovery does not happen at all (`OS2VM_E_DISCOVERY_TIMEOUT`,
> no marker on either node within the full window), and `quic-required` requires
> a peer connect that its own marker contract documents as impossible in a
> single-VM profile. The lane is in neither `test-all` nor CI, which is why it
> rotted unnoticed since ~2026-07-24. Details, workstreams and exit criteria:
> [tasks/TRACK-NETWORK-PROOF-LANES.md](../../tasks/TRACK-NETWORK-PROOF-LANES.md).

## Workflow checklist

1. Extend userspace tests first and run `cargo test --workspace` until green.
2. Execute Miri for host-compatible crates.
3. Refresh Golden Vectors (IDL frames, ABI structs) and bump SemVer when contracts change.
4. Rebuild the Podman development container (`podman build -t open-nexus-os-dev -f podman/Containerfile`) so host tooling matches CI.
5. **Run OS build hygiene checks**: `just diag-os` and `just dep-gate` (catches forbidden dependencies).
6. Run OS smoke coverage via QEMU: `just test-os` (bounded by `RUN_TIMEOUT`, exits on readiness markers).
7. For SMP changes, run both boot lanes sequentially. Select the lane by
   **profile** — the profile declares the topology (harts, icount,
   `REQUIRE_SMP`), and passing a contradicting `SMP=`/`QEMU_NO_ICOUNT=` in the
   environment is a hard error, not a silent override:
   - `just ci-os-smp1` — deterministic gate: `-smp 1` + icount, no
     secondary-hart demands (`[profile.smp1]`).
   - `just ci-os-smp` — real parallelism: `-smp 2`, MTTCG, secondary-hart
     proofs required, bounded retry (`[profile.smp]`).

## Quick commands

- Host unit/property: `just test-host`
- Host E2E: `just test-e2e` (runs `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`, `e2e_policy`)
- QEMU smoke: `RUN_UNTIL_MARKER=1 just test-os` (defaults to `PROFILE=full`; e.g. `just test-os visible-bootstrap` for the visible ladder)
- Phase-gated triage: `RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os` (supported phases and failure output: [os-markers.md](os-markers.md))
- Cold-boot durability (statefs, ADR-0044): two runs — `just test-os` seeds the sentinel on a
  fresh image (`SELFTEST: statefs cold-boot seeded`), then
  `NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1 just test-os` boots against the PRESERVED
  image and requires `SELFTEST: statefs cold-boot persist ok`. Without the `REQUIRE_` flag a
  "seeded" verdict on the second boot would pass silently; with it, broken persistence fails
  the lane. This is the only lane that proves real cold boot rather than soft-reboot replay.
- Miri tiers: `just miri-strict` / `just miri-fs` (tier rules: [os-markers.md](os-markers.md))
- Build hygiene gates: `just diag-os`, `just dep-gate`, `just diag-host` ([build-hygiene.md](build-hygiene.md))
- Full gate before "everything is green": `just test-all`; Make wrapper: `make verify`

The complete just-target catalog (incl. per-TASK proof floors) lives in [os-markers.md](os-markers.md) § "Just targets".

## Readiness contract essentials (RFC-0013)

- `init: up <svc>` means the control-plane handshake completed (spawn + bootstrap channel is live).
- `<svc>: ready` means the service is fully ready to serve its v1 contract.
- Tests MUST NOT treat `init: up` as readiness; missing `<svc>: ready` is a hard failure with an explicit error message.
- The full deterministic UART marker order (50 markers, banner → `SELFTEST: end`) is documented in [os-markers.md](os-markers.md).

## Environment parity essentials

- `make initial-setup` provisions all of the below; `make doctor` (`scripts/check-deps.sh`) verifies a host in seconds and names the exact fix for each gap. Prefer it over eyeballing package lists — package *names* differ per distro and per release (Ubuntu 25.10+ split RISC-V out of `qemu-system-misc`), capabilities do not.
- Toolchain pinned via `rust-toolchain.toml`; targets require `rustup target add riscv64imac-unknown-none-elf`. `just test-all` additionally needs the `miri` component.
- System dependencies: a RISC-V-capable `qemu-system-riscv64`, `capnproto`, `ripgrep` (failure triage in `scripts/qemu-test.sh` and `just deadcode` shell out to `rg`), `just`, and supporting build packages; the Podman container image installs the same set for CI parity.
- **Headless lanes need no display stack; `just start` does.** The interactive path defaults to `GPU_MODE=virgl` in a `-display gtk,gl=on` window, and the virgl proof lane uses `egl-headless`. Both come from host GTK + EGL/virglrenderer, which on Debian/Ubuntu live in packages separate from the emulator (`qemu-system-gui`, `qemu-system-modules-opengl`). CI is deliberately headless and never exercises this path — `make doctor` is what catches a QEMU that boots fine but cannot open a window.
- Do not rely on host-only tools — update `scripts/install-deps.sh` **and** `podman/Containerfile` together when new packages are needed.
- Full detail: [e2e.md](e2e.md) § "Environment parity & prerequisites".

## Test logs

All test/QEMU runs write to `build/logs/<profile>--<timestamp>/` (`latest` symlink can be stale — prefer the newest run directory). See [`docs/testing/run-logs.md`](run-logs.md) for the run-directory layout, the `hypothesis.json` decode grid (H1..H5, H4 = build errors, H4b = build warnings, …), and `just logs-gc [keep]` pruning.

## Document map

Split reference documents (formerly `index.md`):

- [layers.md](layers.md) — full testing-layers reference: kernel/userspace/services layers, end-to-end coverage table, and all per-TASK requirement matrices.
- [os-markers.md](os-markers.md) — scaffold sanity: just targets, manifest-driven proof workflow (profile catalog, adding markers/profiles/phases, `verify-uart`), phase-gated QEMU smoke, Miri tiers, and the full OS-E2E UART marker sequence + VMO split notes.
- [e2e.md](e2e.md) — Policy E2E notes, Remote E2E harness, and environment parity & prerequisites.
- [security.md](security.md) — security testing: `test_reject_*` negative cases, hardening markers, fuzz targets, review checklist.
- [build-hygiene.md](build-hygiene.md) — OS build hygiene gates (`just diag-os`, `just dep-gate`) and house rules.
- [troubleshooting.md](troubleshooting.md) — QEMU/UART triage tips, log knobs, and determinism pointers.

Topic guides:

- [e2e-coverage-matrix.md](e2e-coverage-matrix.md) — feature-by-feature E2E coverage map across host-first and OS-last layers.
- [device-mmio-access.md](device-mmio-access.md) — Device MMIO access tests today + extension plan.
- [network-distributed-debugging.md](network-distributed-debugging.md) — SSOT for network/distributed triage (`qemu-test` proof knobs, `os2vm` phases, packet capture, typed error matrix).
- [proof-manifest.md](proof-manifest.md) — Proof Manifest schema (`source/apps/selftest-client/proof-manifest.toml`), the marker/profile/phase SSOT.
- [display-output-hardening-matrix.md](display-output-hardening-matrix.md) — per-service proof matrix for visible display-output closure.
- [replay-and-bisect.md](replay-and-bisect.md) — Phase-6 replay/bisect workflow, bounded budgets, determinism allowlist operations, **proof-floor evidence map (§9)**, **synthetic bad-bundle reproducer (§10)**, and **single remaining environmental closure step (external CI-runner replay artifact, §11)**.
- [trace-diff-format.md](trace-diff-format.md) — deterministic trace diff classes and machine-readable output contract.
- [evidence-bundle.md](evidence-bundle.md) — normative spec for signed evidence bundles (RFC-0038 Phase 5).
- [bisect-good-drift-regress.json](bisect-good-drift-regress.json) — fixture for the Phase-6 3-commit `good→drift→regress` synthetic bisect smoke (`tools/bisect-evidence.sh ... --synthetic-map=docs/testing/bisect-good-drift-regress.json ...`).
- [trace-diff-fixtures.json](trace-diff-fixtures.json) — fixture corpus for `tools/diff-traces.sh` (exact / extra / missing / reorder / phase-mismatch classes).
