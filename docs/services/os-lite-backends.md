# OS-lite service backend layout

The `os-lite` feature builds the core service daemons using cooperative
mailbox IPC and single-address-space tasks. Since the `*-os` shim crates
have been removed, the runtime pulls the lightweight implementations
directly from the primary service crates:

- `source/services/packagefsd/src/os_lite.rs`
- `source/services/vfsd/src/os_lite.rs`

Both modules are re-exported when `nexus_env="os"` and
`feature="os-lite"` are active, so consumers such as the selftest client
can depend on `packagefsd` and `vfsd` without extra feature wiring. This
keeps the workspace smaller while preserving the existing host
implementations behind the default path.

The cooperative mailbox transport lives in
`userspace/nexus-ipc/src/os_lite.rs`. It inlines the frame-size and
queue-depth limits that used to be part of the `os-mailbox-lite` stub
crate, preserving behaviour for existing tests while avoiding an extra
crate hop.
