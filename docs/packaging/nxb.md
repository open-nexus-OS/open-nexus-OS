# Nexus bundle packaging (`.nxb`)

The v1 loader focuses on executing a single embedded ELF payload while the
bundle manager → execd handoff is being designed. To keep the packaging story
unambiguous we standardise the on-disk layout now so tools and docs are ready
when the service RPCs land.

## Layout

An `.nxb` directory contains two files:

```
<bundle>.nxb/
├── manifest.json
└── payload.elf
```

- `manifest.json` follows the minimal schema used by the policy stub:
  `{"name":"demo.hello","version":"0.0.1","required_caps":[],"publisher":"dev","sig":""}`.
  Signing and policy enforcement happen elsewhere; the field is reserved so the
  layout stays forward compatible.
- `payload.elf` is the ELF64/RISC-V binary. In v1 the ELF is embedded directly
  into `execd` and exposed via `exec_payloads::HELLO_ELF`; future revisions will
  source it from `bundlemgrd`.

## Authoring bundles

The helper `tools/nxb-pack` crate creates the directory for you:

```
cargo run -p nxb-pack -- path/to/app.elf out/demo.hello.nxb
```

The tool copies the input ELF into `payload.elf` and writes the default manifest
(no signing, no extra capabilities). For prototypes you can edit
`manifest.json` manually to tweak the advertised capabilities before running
policy checks on host.

## Relationship to the loader roadmap

- **v1 (this milestone):** `execd::exec_hello_elf()` embeds a prebuilt ELF via a
  static byte array. Bundles are not fetched dynamically yet; `.nxb` packaging is
  documented so tests and tooling stay aligned.
- **v1.1:** `bundlemgrd` will expose a read-only VMO handle containing the ELF
  bytes for the requested bundle. `execd` will call the new RPC, validate the
  manifest against policy, and stream the bytes into the loader, reusing the same
  layout described above.

Keeping the packaging deterministic makes it trivial to stage host tests and to
assert the policy/execd handshake once the service pipeline is wired end-to-end.
