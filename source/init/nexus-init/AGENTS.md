# Nexus Init Coding Notes

This directory houses the shared `nexus-init` crate used by both the host test
runner and the lightweight OS images.

## Backend split

- Keep the `std_server` module byte-for-byte compatible with the existing host
  runtime. The library root selects `std_server` whenever `nexus_env="host"`
  or the `os-lite` feature is disabled.
- The `os_lite` module is compiled only for `nexus_env="os"` with the
  `os-lite` feature. Start with the cooperative bootstrap stub that emits the
  UART markers `init: start` and `init: ready` and yields with
  `nexus_abi::yield_()`. Future stages will extend this backend to spawn core
  services and distribute capabilities.

## Staged migration plan

1. Preserve the host code path during every refactor and keep the UART markers
   `packagefsd: ready`, `vfsd: ready`, and the `SELFTEST` probes untouched.
2. Grow the os-lite backend incrementally (readiness notification, IPC wiring,
   service spawning) while guarding new code behind `feature = "os-lite"`.
3. Once the os-lite runtime reaches parity, flip the boot image to launch it
   instead of the old stage0 shim.

## Testing expectations

- Host builds must continue to pass `cargo test --workspace`.
- OS images run under `just test-os` must still show the init markers followed
  by the packagefsd/vfsd/selftest readiness prints.

Document updates should note the dual-backend approach and call out when new
capabilities are wired into the os-lite path.
