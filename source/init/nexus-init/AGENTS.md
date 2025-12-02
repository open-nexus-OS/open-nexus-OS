# Nexus Init Coding Notes

This directory houses the shared `nexus-init` crate used by both the host test
runner and the lightweight OS images. Every file in this tree must keep the
standard CONTEXT header (see `docs/standards/DOCUMENTATION_STANDARDS.md`);
adjust fields when behaviour changes and ensure the ADR reference (`docs/adr/0017-service-architecture.md`) stays accurate.

## Backend split

- Keep the `std_server` module byte-for-byte compatible with the existing host
  runtime. The library root selects `std_server` whenever `nexus_env="host"`
  or the `os-lite` feature is disabled.
- The `os_lite` module is compiled only for `nexus_env="os"` with the
  `os-lite` feature. Start with the cooperative bootstrap stub that emits the
  UART markers `init: start` and `init: ready` and yields with
  `nexus_abi::yield_()`. Future stages will extend this backend to spawn core
  services and distribute capabilities.

## Stage 2 (os-lite bootstrap)

- Grow the cooperative bootstrap so it launches the seven core services in a
  deterministic order (`keystored` through `execd`). Each launch should set the
  IPC default target first, build the service's readiness notifier, and then
  enter its `service_main_loop` using the lite mailbox transport.
- The readiness notifier must emit `init: up <service>` exactly once. These
  markers land before the legacy `init: ready` print and end up bracketed by
  the downstream `packagefsd: ready` and `vfsd: ready` UART markers during boot.
- After every readiness callback fires, yield back to the scheduler with
  `nexus_abi::yield_()` so other cooperative tasks can observe progress.
- Service failures should be logged as `init: fail <service>: ...` without
  aborting init. The long-term plan is to replace the sequential loop with true
  task spawning once lite IPC gains process support.

## Stage 3 (service tasks)

- Each service now launches through `spawn_service`, which configures the
  default IPC target, provisions a dedicated address space and stack via
  `nexus_abi::{as_create, as_map}`, records the service runtime descriptor, and
  calls `nexus_abi::spawn`. The returned `SpawnHandle` keeps the allocated
  address space and stack VMOs rooted so follow-on prompts can wire up proper
  teardown.
- The bootstrap capability in slot `0` is transferred to the child with SEND
  rights via `nexus_abi::cap_transfer` before waiting for readiness.
- A shared trampoline (`service_task_entry`) pops the registered descriptor and
  invokes the existing `service_main_loop`, preserving UART readiness prints.
- The parent yields after every spawn so the child task can announce
  `init: up <service>`; failures still log `init: fail <service>: …` and
  bootstrap continues.
- Keep the helper boundaries clean so future stages can hand out
  rights-filtered capability sets per service and tighten stack/VMO lifecycle
  management once destruction hooks land.

## Staged migration plan

1. Preserve the host code path during every refactor and keep the UART markers
   `packagefsd: ready`, `vfsd: ready`, and the `SELFTEST` probes untouched.
2. Grow the os-lite backend incrementally (readiness notification, IPC wiring,
   service spawning) while guarding new code behind `feature = "os-lite"`.
3. Once the os-lite runtime reaches parity, flip the boot image to launch the
   os-lite backend by default.

## Testing expectations

- Host builds must continue to pass `cargo test --workspace`.
- OS images run under `just test-os` must still show the init markers followed
  by the packagefsd/vfsd/selftest readiness prints.

Document updates should note the dual-backend approach and call out when new
capabilities are wired into the os-lite path.
