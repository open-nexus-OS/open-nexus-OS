<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Scaffold sanity and OS-E2E marker sequence

QEMU smoke scaffolding and the canonical UART marker contract: just targets, the manifest-driven proof workflow (profiles/markers/phases), phase-gated smoke, Miri tiers, and the full OS-E2E marker sequence with VMO split notes. Split out of the former `docs/testing/index.md`; see [README.md](README.md) for the entry point.

## Scaffold sanity

Run the QEMU smoke test to confirm the UART marker sequence reaches
`SELFTEST: e2e exec-elf ok`. Keep `RUN_UNTIL_MARKER=1` to exit early once markers are
seen and ensure log caps are in effect. `just test-os` wraps
`scripts/qemu-test.sh`, so the same command exercises the minimal exec path.

### Just targets

- Host unit/property: `just test-host`
- Host E2E: `just test-e2e` (runs `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`, `e2e_policy`)
- DSoftBus mux requirement suites (`TASK-0020`): `just test-dsoftbus-mux`
- DSoftBus QUIC host requirement suites (`TASK-0021`): `just test-dsoftbus-quic`
- DSoftBus full host regression: `just test-dsoftbus-host`
- nx CLI host proof suite (`TASK-0045`): `cargo test -p nx -- --nocapture`
- Config v1 host proof floor (`TASK-0046`):
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - stable reject classification for unknown/type/depth/size failures
    - deterministic layered merge order and JSON-only authoring contract
    - semantic parity between `configd` views and `nx config effective --json`
    - honest 2PC commit/abort/rollback state transitions with unchanged-version evidence on failure
    - deterministic CLI exit/JSON/file-effect contracts under `nx config`
- Policy as Code v1 host proof floor (`TASK-0047`):
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - single live policy root under `policies/` with `recipes/policy/` non-authoritative
    - deterministic `PolicyVersion` for equivalent validated inputs
    - stable `test_reject_*` classes for invalid/oversize/ambiguous/traversal/trace-budget cases
    - deterministic `policies/manifest.json` validation and fail-closed missing/mismatch handling
    - bounded explain traces and dry-run/learn non-bypass semantics
    - authenticated/current-version mode changes and stale/unauthorized rejects
    - Config v1 `policy.root` effective-snapshot carriage into the `configd::ConfigConsumer` policy reload path
    - external `policyd` host frame operations for version/eval/mode get/mode set
    - first adapter parity plus service-facing `policyd` check-frame cutover through the unified authority
    - `nx policy` deterministic exit/JSON contracts under the existing `tools/nx` binary, including explicit `mode` preflight-only output
- UI host renderer snapshot proof floor (`TASK-0054` / `RFC-0046`):
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - BGRA8888 byte order and 64-byte frame stride
    - deterministic primitive pixels and bounded damage semantics
    - fixture-font text without host font discovery or locale fallback
    - canonical BGRA golden comparison with PNG metadata ignored
    - fail-closed update/path traversal/input reject classes
    - no host-only fake OS/QEMU success markers
- Input v1.0a host-core proof floor (`TASK-0252` / `RFC-0052`):
  - `cargo test -p input_v1_0_host -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - USB-HID boot keyboard/mouse parsing emits deterministic logical events
    - touch normalization preserves ordered `down -> move* -> up`
    - shared base keymaps cover deterministic `us`, `de`, `jp`, `kr`, and `zh` vectors without locale probing
    - repeat timing is driven by injected monotonic time rather than wall-clock behavior
    - pointer acceleration is monotonic, bounded, and safe for extreme deltas
    - `test_reject_*` coverage exists for malformed HID/touch inputs and invalid repeat/accel configuration
  - no QEMU markers or `SELFTEST: ... ok` strings are part of 0252 closure; live-input markers remain `TASK-0253` scope
- Input v1.0b live-input proof floor (`TASK-0253` / `RFC-0053`):
  - `cargo test -p hidrawd -- --nocapture`
  - `cargo test -p touchd -- --nocapture`
  - `cargo test -p inputd -- --nocapture`
  - `cargo test -p settingsd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
  - proves Soll-Verhalten for the currently landed live-input slice:
    - `hidrawd` keyboard/mouse ingest rejects malformed boot-protocol input deterministically
    - `touchd` deterministic synthetic touches remain bounded and normalized
    - `inputd` routes keyboard, pointer, and touch through `windowd` authority with bounded queues
    - IME show/hide hooks remain bounded stubs only
    - `visible-bootstrap` marker verification covers the in-process `hidrawd|touchd -> inputd -> windowd` proof path under `verify-uart`
  - active hardening/debug order for the remaining live pointer closure:
    - derive visible QEMU input devices from the selected SystemUI profile manifest instead of hardcoding mouse+tablet together,
    - run source-isolated proofs first (`mouse-only`, `tablet-only`, `keyboard-only`) to identify the failing service boundary honestly,
    - only then rerun the combined visible-bootstrap lane for modern desktop-style mixed mouse+touch behavior,
    - keep `visible-bootstrap` as the guest-side/QMP-injected proof lane only; real host-pointer closure belongs to `make run` / `just start`,
    - for the interactive lane, require sustained cursor movement evidence on the real GTK/QEMU path rather than accepting a single first-move event as closure.
  - does **not** yet prove separate OS daemon startup/device wiring for `hidrawd`, `touchd`, and `inputd`; broad closure gates remain task-controlled
- RFC-0054 gate hardening floor (`TASK-0253` in-progress live-daemon slice):
  - **General capability/routing/IPC gates**
    - `cargo test -p nx --test interactive_os_startup`
    - proves deterministic owner-chain contracts as static guards:
      - `init-lite` transfers dedicated `input_req` / `input_rsp` caps to `hidrawd` and `inputd`,
      - routing contract exposes explicit `inputd`/`hidrawd` lookup entries,
      - `inputd` keeps a deterministic named-route -> slot-fallback posture for startup resilience.
  - **Input-specific gates**
    - `cargo test -p virtio-input -- --nocapture`
    - proves virtio-input role detection is bounded and tolerant of optional config-bit absence (keyboard-safe default).
    - `cargo test -p hidrawd -- --nocapture`
    - proves bounded ingest and deterministic event mapping remain green while OS-lite owner-loop posture evolves.
  - **Focused OS startup gate**
    - `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`
    - proves `hidrawd`/`touchd`/`inputd` startup markers in canonical order without waiting for full end-to-end closure.
- QEMU smoke: `RUN_UNTIL_MARKER=1 just test-os` (defaults to `PROFILE=full`)
- QEMU smoke (DHCP requested): `just ci-os-dhcp` (PROFILE-driven; replaces the deleted `test-os-dhcp`)
- QEMU smoke (Strict DHCP gate): `just ci-os-dhcp-strict` (PROFILE-driven; replaces the deleted `test-os-dhcp-strict`)
- DSoftBus 2-VM harness (TASK-0005): `just ci-os-os2vm` (PROFILE-driven; or `just os2vm` for the raw 2-VM driver)
- Crashdump v1 host proofs (TASK-0018):
  - `cargo test -p crash -- --nocapture`
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p minidump-host -- --nocapture`
  - `cargo test -p statefsd -- --nocapture`
- DSoftBus 2-VM harness + PCAP: `just test-dsoftbus-2vm-pcap` (or `just os2vm-pcap`)
  - Expected remote markers on success: `SELFTEST: remote resolve ok`, `SELFTEST: remote query ok`, `SELFTEST: remote pkgfs stat ok`, `SELFTEST: remote pkgfs open ok`, `SELFTEST: remote pkgfs read step ok`, `SELFTEST: remote pkgfs close ok`, `SELFTEST: remote pkgfs read ok`, and `dsoftbusd: remote packagefs served`
- Full gate (recommended before “everything is green”): `just test-all`
  - Includes `fmt-check`, `lint`, `deny-check`, host tests, Miri tiers, `arch-check`, and QEMU selftests.
- Make convenience wrapper: `make verify`
  - Delegates to `just` gates (`diag-host`, `test-host`, `test-e2e`, `dep-gate`, `diag-os`, `test-os`).
  - Optional SMP dual-mode extension: `REQUIRE_SMP_VERIFY=1 make verify`.

### Manifest-driven workflow (TASK-0023B Phase 4)

The QEMU marker ladder, harness profile catalog, and runtime selftest profile catalog live under **`source/apps/selftest-client/proof-manifest/manifest.toml`** and its included `markers/`, `profiles/`, and `phases.toml` files. This proof-manifest tree is the single source of truth (SSOT). `scripts/qemu-test.sh`, `tools/os2vm.sh`, the `selftest-client` build, and the `nexus-proof-manifest` host CLI all read from it; nothing else is allowed to declare markers, profile env, or phase order.

#### Profile catalog

| Kind     | Profile         | Driver                          | Purpose |
|---       |---              |---                              |---|
| Harness  | `full`          | `scripts/qemu-test.sh`          | Default 12-phase ladder (`just test-os` / `just ci-os-full`). |
| Harness  | `visible-bootstrap` | `scripts/qemu-test.sh`      | TASK-0055B/0055C/0056/TASK-0057 visible `ramfb`, SystemUI, v2a, live-input, and Minimal DisplayServer v0 asset marker ladder (`just test-os visible-bootstrap`); not a SystemUI start profile or screenshot proof. |
| Harness  | `smp`           | `scripts/qemu-test.sh`          | SMP-only marker contract; consumed by `just ci-os-smp` (SMP=2 strict + SMP=1 parity). |
| Harness  | `dhcp`          | `scripts/qemu-test.sh`          | DHCP requested; deterministic fallback allowed (`just ci-os-dhcp`). |
| Harness  | `dhcp-strict`   | `scripts/qemu-test.sh`          | DHCP must bind (`just ci-os-dhcp-strict`); extends `dhcp`. |
| Harness  | `quic-required` | `scripts/qemu-test.sh`          | QUIC required; `transport selected tcp` is forbidden (`just ci-os-quic`). |
| Harness  | `supply-chain`  | `scripts/qemu-test.sh`          | Supply-chain marker ladder required (`just test-os supply-chain`). |
| Harness  | `os2vm`         | `tools/os2vm.sh`                | Cross-VM DSoftBus harness (`just ci-os-os2vm`). |
| Runtime  | `bringup`       | `os_lite::profile` (in-OS)      | Boots `bringup` + `end` phases only (`SELFTEST_PROFILE=bringup`). |
| Runtime  | `quick`         | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `mmio` → `end`. |
| Runtime  | `ota`           | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `ota` → `end`. |
| Runtime  | `net`           | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `mmio` → `routing` → `net` → `end`. |
| Runtime  | `none`          | `os_lite::profile` (in-OS)      | `bringup` + `end` only — empty middle. |

Harness profiles live under `[profile.<name>]` with `runner` + optional `extends` + `env` keys. Runtime profiles set `runtime_only = true` and declare `phases = [...]` (subset of the 12 declared `[phase.X]` entries); skipped phases emit a single `dbg: phase X skipped` breadcrumb instead of any `SELFTEST:` markers.

#### Adding a new marker

1. Append a `[marker."<literal>"]` entry to `proof-manifest.toml` with `phase = "<name>"` and any `emit_when = { profile = "..." }` / `forbidden_when = { profile = "..." }` gates.
2. Re-run `cargo build -p selftest-client` (the `build.rs` regenerates `markers_generated.rs` with a `pub(crate) const M_<KEY>: &str = "<literal>";` constant).
3. In the emitting site, reference the constant: `crate::markers::emit_line(crate::markers::M_<KEY>);`. Do not hand-write the literal — `arch-gate` Rule 3 fails the build if any `SELFTEST:` / `dsoftbusd:` / `dsoftbus:` literal appears outside `markers.rs` + `markers_generated.rs`.
4. Add a `cargo test -p nexus-proof-manifest` reject test if the marker carries new gating semantics.

#### Adding a new harness profile

1. Append `[profile.<name>]` with `runner = "scripts/qemu-test.sh"` (or your driver), optional `extends = "<parent>"`, and `env = { ... }`. Use `extends` for inheritance — child entries shadow parent keys; cycles are rejected at parse time.
2. Add a `just ci-<flavor>` recipe in the `# CI matrix` section of `justfile` that invokes `just test-os PROFILE=<name>` (or your alternate runner). Do **not** put `REQUIRE_*` env literals in the recipe body — `arch-gate` Rule 6 will fail. All env wiring belongs in the manifest.
3. Verify: `nexus-proof-manifest list-env --profile=<name>` prints the resolved env; `nexus-proof-manifest list-markers --profile=<name>` prints the expected marker ladder.

#### Adding a new runtime profile

1. Append `[profile.<name>]` with `runtime_only = true` and `phases = ["...", "..."]` (subset of declared phases). The parser rejects unknown phase names and rejects `runner` keys on runtime-only profiles.
2. Add a `Profile::<Name>` variant to `source/apps/selftest-client/src/os_lite/profile.rs` and wire it into `Profile::from_kernel_cmdline_or_default` + `Profile::includes`.
3. Add a `ci-os-runtime-<name>` recipe that sets `SELFTEST_PROFILE=<name>` at build time (the env is read via `option_env!`, so the value is baked into the binary; `build.rs` already advertises `cargo:rerun-if-env-changed=SELFTEST_PROFILE`).

#### Adding a new phase

1. Update [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) and append a `[phase.<name>] order = N` entry to `proof-manifest.toml`.
2. Add `source/apps/selftest-client/src/os_lite/phases/<name>.rs` and register it in `os_lite/phases/mod.rs`.
3. Extend `PhaseId` in `os_lite/profile.rs` and the `run_or_skip!` chain in `os_lite/mod.rs` (declarative; no logic).
4. Add a `dbg: phase <name> skipped` marker entry to `proof-manifest.toml` so runtime profiles can declaratively skip the phase.

#### Deny-by-default analyzer (P4-09)

After every QEMU pass `scripts/qemu-test.sh` invokes `nexus-proof-manifest verify-uart --profile=<name> --uart=uart.log`. Any marker that is not in the profile's expected set, or that the profile lists as forbidden, fails the run with exit 1. Set `PM_VERIFY_UART=0` only as a temporary escape hatch (will be required by P4-10 closure / Phase-5).

### Phase-gated QEMU smoke (triage helper)

For faster triage (RFC‑0014 Phase 2), you can stop QEMU early after a named **phase** and only
validate markers up to that phase:

- Supported phases: `bring-up`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`

Examples:

```bash
# Stop after core service bring-up (init + core *: ready markers)
RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os

# Stop after OTA smoke (stage → switch → health → rollback)
RUN_PHASE=ota RUN_TIMEOUT=190s just test-os
```

On failure, the harness prints:

- `first_failed_phase=<name>`
- `missing_marker='<marker>'`
- a **bounded** UART excerpt scoped to the failed phase.

### Network/distributed debugging (SSOT)

Canonical networking proof semantics and knobs now live in:

- `docs/testing/network-distributed-debugging.md` (SSOT)

Keep this file focused on the global testing workflow. For DHCP/DSoftBus proof gates, packet evidence expectations, and `os2vm` typed failure triage, use the SSOT document.

### Miri tiers

- Strict (no IO): run on crates without filesystem/network access.
  - Example: `just miri-strict` (uses `MIRIFLAGS='--cfg nexus_env="host"'`).
- FS-enabled: for crates that legitimately touch the filesystem or env.
  - Example: `just miri-fs` (uses `MIRIFLAGS='-Zmiri-disable-isolation --cfg nexus_env="host"'`).
- Under `#[cfg(miri)]`, keep property tests lightweight (lower case count, disable persistence) to avoid long runtimes.

## OS-E2E marker sequence and VMO split

The os-lite `nexus-init` backend is responsible for announcing service
bring-up. The OS smoke path emits a deterministic sequence of UART markers that
the runner validates in order.

Readiness contract (RFC-0013):

- `init: up <svc>` means the control-plane handshake completed (spawn + bootstrap channel is live).
- `<svc>: ready` means the service is fully ready to serve its v1 contract.
- Tests MUST NOT treat `init: up` as readiness; missing `<svc>: ready` is a hard failure with an explicit error message.
- Low-effort readiness gates may query `logd` for `*: ready` markers when a service lacks a dedicated ready RPC (stopgap until readiness RPCs exist).

Additional kernel selftest markers may appear before `init: start`:

- `KSELFTEST: spawn reasons ok`
- `KSELFTEST: resource sentinel ok`

SMP-gated markers (enabled only with `REQUIRE_SMP=1` and `SMP>=2`):

- `KINIT: cpu1 online`
- `KSELFTEST: smp online ok`
- `KSELFTEST: ipi counterfactual ok`
- `KSELFTEST: ipi resched ok`
- `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
- `KSELFTEST: test_reject_offline_cpu_resched ok`
- `KSELFTEST: work stealing ok`
- `KSELFTEST: test_reject_steal_above_bound ok`
- `KSELFTEST: test_reject_steal_higher_qos ok`

Memory pressure note (until TASK-0228):

- Boot runs may still hit allocator exhaustion (`ALLOC-FAIL`) or late spawn failures due to memory pressure.
- This is expected to be fully addressed by the cooperative OOM watchdog in
  `tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md`.
- Until then, use the SpawnFailReason markers and readiness gates to diagnose failures quickly.

Marker order:

1. `neuron vers.` – kernel banner
2. `init: start` – init process begins bootstrapping services
3. `init: start keystored` – os-lite init launching the keystore daemon
4. `init: up keystored` – os-lite init observed the keystore daemon reach readiness
5. `init: start policyd` – os-lite init launching the policy daemon
6. `init: up policyd` – os-lite init observed the policy daemon reach readiness
7. `init: start samgrd` – os-lite init launching the service manager
8. `init: up samgrd` – os-lite init observed the service manager reach readiness
9. `init: start bundlemgrd` – os-lite init launching the bundle manager
10. `init: up bundlemgrd` – os-lite init observed the bundle manager reach readiness
11. `init: start packagefsd` – os-lite init launching the package FS daemon
12. `init: up packagefsd` – os-lite init observed the package FS daemon reach readiness
13. `init: start vfsd` – os-lite init launching the VFS dispatcher
14. `init: up vfsd` – os-lite init observed the VFS dispatcher reach readiness
15. `init: start execd` – os-lite init launching the exec daemon
16. `init: up execd` – os-lite init observed the exec daemon reach readiness
17. `init: ready` – init completed baseline bring-up
18. `keystored: ready` – keystore daemon ready (emitted by service process)
19. `policyd: ready` – policy daemon ready
20. `samgrd: ready` – service manager daemon ready
21. `bundlemgrd: ready` – bundle manager daemon ready
22. `packagefsd: ready` – package file system daemon registered
23. `vfsd: ready` – VFS dispatcher ready to serve requests
24. `execd: ready` – exec daemon ready (may be stubbed during bring-up)
25. `execd: elf load ok` – exec syscall mapped the embedded ELF into the child address space
26. `child: hello-elf` – spawned task from the ELF payload started and yielded control back to the kernel
27. `SELFTEST: e2e exec-elf ok` – selftest client observed the ELF loader path end-to-end
28. `child: exit0 start` – demo payload exercising `exit(0)` began execution
29. `execd: child exited pid=… code=0` – selftest client observed the exit status (marker name kept for compatibility)
30. `SELFTEST: child exit ok` – selftest client observed the lifecycle markers
31. `SELFTEST: policy allow ok` – simulated allow path succeeded via policy check
32. `SELFTEST: policy deny ok` – simulated denial path emitted for `demo.testsvc`
33. `SELFTEST: ipc payload roundtrip ok` – kernel IPC v1 payload copy-in/out roundtrip succeeded
34. `SELFTEST: ipc deadline timeout ok` – kernel IPC v1 deadline semantics: past deadline returns `TimedOut` deterministically
35. `SELFTEST: nexus-ipc kernel loopback ok` – `nexus-ipc` OS backend exercised kernel IPC v1 syscalls (loopback on bootstrap endpoint)
36. `SELFTEST: ipc routing ok` – `nexus-ipc` resolved a named service target via the init-lite routing responder
37. `SELFTEST: vfs stat ok` – VFS over IPC: stat succeeded
38. `SELFTEST: vfs read ok` – VFS over IPC: open/read succeeded
39. `SELFTEST: vfs ebadf ok` – VFS over IPC: EBADF behavior verified
40. `logd: ready` – logd RAM journal ready for APPEND/QUERY/STATS
41. `SELFTEST: log query ok` – selftest client queried logd and verified records
42. `SELFTEST: core services log ok` – core services (samgrd/bundlemgrd/policyd/dsoftbusd) emit structured logs to logd (verified via bounded QUERY + STATS delta)
43. `execd: minidump written /state/crash/child.demo.minidump.nmd` – crash artifact metadata validated and accepted
44. `execd: crash report pid=... code=42 name=demo.minidump` – execd observed non-zero exit and emitted crash report
45. `SELFTEST: crash report ok` – selftest client verified crash report via logd query
46. `SELFTEST: minidump ok` – selftest client validated artifact + report path end-to-end
47. `SELFTEST: minidump forged metadata rejected` – forged metadata rejected fail-closed
48. `SELFTEST: minidump no-artifact metadata rejected` – report without artifact bytes rejected fail-closed
49. `SELFTEST: minidump mismatched build_id rejected` – inconsistent build metadata rejected fail-closed
50. `SELFTEST: end` – concluding marker from the selftest client

### OS E2E: exec-elf (service path)

- Run `RUN_UNTIL_MARKER=1 just test-os` to exercise the service-integrated loader flow on QEMU.
- Confirm the UART log contains `execd: elf load ok`, `child: hello-elf`, the lifecycle markers (`child: exit0 start`, `execd: child exited pid=… code=0`, `SELFTEST: child exit ok`), and `SELFTEST: e2e exec-elf ok` before ending the run.
- Host coverage mirrors the flow via `cargo test -p nexus-loader` for loader unit tests and the bundle manager IPC round-trips in
  `cargo test -p tests/e2e`.
