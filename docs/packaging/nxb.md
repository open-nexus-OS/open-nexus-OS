# Nexus bundle packaging (`.nxb`)

**Status**: Active (updated 2026-01-15)  
**Canonical source**: ADR-0020 (manifest format decision)

The loader v1.1 milestone wires `bundlemgrd` and `execd` together so installed
bundles can be executed via the same assets used during packaging. Keeping the
layout deterministic makes it trivial to stage artifacts in host tests and on
the OS image.

## Layout

An `.nxb` directory contains two files:

```text

<bundle>.nxb/
├── manifest.nxb
└── payload.elf

```text

- **`manifest.nxb`**: Canonical, deterministic bundle manifest (Cap'n Proto binary).
  - **Format**: Cap'n Proto (`tools/nexus-idl/schemas/manifest.capnp`)
  - **Deterministic**: Same manifest data → same binary output (signable)
  - **Versionable**: Schema v1.0 (core fields), v1.1 (digest/size), v2.0+ (future)
  - **Replaces**: Old JSON/TOML formats (drift resolved in TASK-0007)
  
- **`payload.elf`**: ELF64/RISC-V binary. In v1.1 the same payload is staged

  in `bundlemgrd`'s artifact store so the daemon can serve it to `execd` during
  `getPayload`.

## Authoring bundles

The helper `tools/nxb-pack` crate creates the directory for you:

```bash

# From TOML source (human-editable)

cargo run -p nxb-pack -- --toml manifest.toml path/to/app.elf out/demo.hello.nxb

# Quick mode (generates default manifest)

cargo run -p nxb-pack -- path/to/app.elf out/demo.hello.nxb

```text

**Workflow**:
1. **Input**: `manifest.toml` (TOML, human-editable)
2. **Compile**: `nxb-pack` → `manifest.nxb` (Cap'n Proto binary)
3. **Package**: Copy `payload.elf` + `manifest.nxb` to output directory

**Example `manifest.toml`**:

```toml

name = "demo.hello"
version = "1.0.0"
abilities = ["ohos.ability.MainAbility"]
caps = ["ohos.permission.INTERNET"]
min_sdk = "1.0.0"
publisher = "00000000000000000000000000000000"
sig = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"

```text

**Output `manifest.nxb`**: Binary Cap'n Proto encoding (deterministic, signable)

## Loader handshake

Once a bundle is installed, `execd::exec_elf(bundle, argv, env, policy)` calls
the new `bundlemgrd.getPayload(name)` RPC. The daemon validates the install,
resolves the `payload.elf` bytes (splitting across frames when necessary), and
returns them to `execd`. The service writes the bytes into a staging VMO, runs
`nexus_loader::load_with`, and maps each PT_LOAD segment into a fresh Sv39
address space. W^X is enforced twice: the loader rejects write+execute segments
and the kernel refuses conflicting protection flags at `as_map` time. A private
stack is provisioned via `StackBuilder`, argv/env tables are copied in, and
`spawn` launches the child process.

After installation completes `bundlemgrd` also publishes the bundle to
`packagefsd`. Files are exposed under `/packages/<name>@<version>/...` and the
alias `pkg:/<name>/...` resolves to the active version. This read-only view is
used by the userspace VFS service (`vfsd`) and is available to other services
via the `nexus-vfs` client crate.

`userspace/exec-payloads` exposes the same manifest bytes and canonical payload
used by `tools/nxb-pack`. This keeps selftests, host fixtures, and the
packaging toolchain aligned.
