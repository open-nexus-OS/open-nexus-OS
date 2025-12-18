# Nexus bundle packaging (`.nxb`)

The loader v1.1 milestone wires `bundlemgrd` and `execd` together so installed
bundles can be executed via the same assets used during packaging. Keeping the
layout deterministic makes it trivial to stage artifacts in host tests and on
the OS image.

## Layout

An `.nxb` directory contains two files:

```
<bundle>.nxb/
├── manifest.toml
└── payload.elf
```

- `manifest.toml` follows the minimal schema used by the policy stub:
  `name = "demo.hello"`, `version = "0.0.1"`, `caps = []`,
  `abilities = []`, `publisher = "dev"`, `sig = ""`.
  Signing and policy enforcement happen elsewhere; the field is reserved so the
  layout stays forward compatible. Long-term, `.nxb` packaging participates in
  the hybrid security root (verified boot + signed bundles/packages + policy)
  described in `docs/agents/VISION.md`.
- `payload.elf` is the ELF64/RISC-V binary. In v1.1 the same payload is staged
  in `bundlemgrd`'s artifact store so the daemon can serve it to `execd` during
  `getPayload`.

## Authoring bundles

The helper `tools/nxb-pack` crate creates the directory for you:

```
cargo run -p nxb-pack -- path/to/app.elf out/demo.hello.nxb
```

The tool copies the input ELF into `payload.elf` and writes a default manifest
in TOML format. For prototypes you can edit `manifest.toml` manually to tweak
the advertised capabilities before running policy checks on host.

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
