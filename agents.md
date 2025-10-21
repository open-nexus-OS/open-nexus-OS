# Codex Agents Guide for Open Nexus OS

## Purpose

Codex frequently contributes to `open-nexus-OS`. This guide captures the expectations for staged refactors, particularly when targeting the lightweight OS runtime. Follow this document before drafting prompts or landing code so that the resulting changes remain small, testable, and consistent with existing architecture.

---

## General Workflow

1. **Stay on feature branches.** Never work directly on `main`. Start every effort by creating (or switching to) a dedicated `feat/` branch; keep commits focused and logical.
2. **Respect `nexus_env` splits.** Host/vector code lives under `nexus_env = "host"`; OS paths use `nexus_env = "os"`. New functionality must honor this split instead of introducing new cfgs ad hoc.
3. **Favour incremental steps.** Large rewrites should be decomposed into prompts that each produce a runnable tree (tests passing, UART markers intact). Avoid sprawling edits.
4. **No kernel edits unless requested.** Userland and service layers are fair game; the kernel stays untouched by default.
5. **Testing discipline.** At the end of every prompt, run the minimal necessary checks (workspace `cargo test`, targeted e2e harness, or UART marker verification). Document expected markers in the prompt itself.

---

## Staged Refactor Pattern (os-lite services)

When migrating a std-based service to an os-lite split:

1. **Module split:** Introduce `std_server.rs` (existing logic) and `os_lite.rs` (new path) behind cfg gates. Keep host behaviour identical.
2. **Skeleton first:** Start `os_lite` as a stub emitting readiness markers. Do not remove the old implementation yet.
3. **Incremental wiring:** Port service responsibilities one feature at a time (e.g. readiness, IPC transport, spawning) with tests after each step.
4. **Enablement:** Only when parity is achieved, flip the entrypoint or feature flag that makes `os_lite` active for the OS build.
5. **Cleanup:** Remove superseded crates/binaries (e.g. `stage0-init-os`) once the new path is stable.

Note: os-lite service backends are now embedded in `packagefsd` and `vfsd`; the old `*-os` crates were removed. The mailbox prototype was superseded by `nexus-ipc`'s os-lite transport.

---

## Prompt Structure Checklist

Every Codex prompt should:

- Identify scope, constraints, and acceptance criteria up front.
- List steps in execution order (A, B, C…) with explicit file edits and commit messages.
- Include testing/postflight instructions that verify success.
- Reference relevant docs/markers so reviewers know what to inspect.

Copy the format from previous prompts (section headers, fenced code for commands, etc.) and adjust the details for the current task.

---

## Readiness Markers

Core OS services must emit deterministic UART markers:

- `packagefsd: ready`
- `vfsd: ready`
- `SELFTEST: vfs stat ok`
- `SELFTEST: vfs read ok`
- `SELFTEST: vfs ebadf ok`

Do not change these without updating scripts, postflight tooling, and docs in the same prompt.

---

## Staged migration plan

1. Preserve the host code path during every refactor and keep the UART markers
   `packagefsd: ready`, `vfsd: ready`, and the `SELFTEST` probes untouched.
2. Grow the os-lite backend incrementally (stage 1 stub ✅, stage 2 sequential
   service bootstrap ✅, stage 3 task spawning ✅), guarding new code behind
   `feature = "os-lite"`.
3. Stage 3 moves each core service into its own task via `nexus_abi::spawn`
   and `cap_transfer`, provisioning a dedicated address space/stack per task
   while reusing the existing service loops through the shared init
   trampoline. `SpawnHandle` now retains those allocations for future teardown
   work, and the next increments will focus on refining capability rights per
   service.
4. Stage 4 locks down bootstrap capability hygiene for the os-lite path:
   init closes its copy of slot `0` immediately after `cap_transfer` succeeds,
   each transfer uses the narrow `Rights::SEND` mask, and the bootstrap loop
   queues every `SpawnHandle` for a teardown pass that destroys the parent
   address-space and stack handles once readiness fires. The teardown helper
   tolerates `Unsupported`/`InvalidSyscall`/`CapabilityDenied` results so the
   cleanup remains idempotent while kernel hooks land. These changes ensure the
   parent no longer retains bootstrap rights or leaked VMOs after Stage 3.
5. Once the os-lite runtime reaches parity, flip the boot image to launch it
   instead of the old stage0 shim.

---

## Stage 4 (capability hygiene & teardown)

- After transferring the bootstrap capability (slot 0) into each service with
  SEND rights, init relinquishes its own reference and later calls
  `cap_close(0)` to drop any lingering parent ref.
- Each spawned service retains a `SpawnHandle` whose Drop releases the stack
  VMO and destroys the address space (`as_destroy`) when available on the
  target. Unsupported/denied results are tolerated and logged as warnings.
- At the end of the bootstrap sequence, `teardown_services` drops all retained
  `SpawnHandle`s and calls the bootstrap-cap release helper before `init: ready`.

---

## Contact

When in doubt, leave a TODO comment or open a follow-up prompt describing uncertainties (e.g. capability distribution strategy). Avoid speculative implementations without validation guidance.
