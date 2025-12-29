# Nexus Bundle (`.nxb`) Format

NOTE (drift): This document describes an older, tar-based “bundle” concept. The current
implementation used by the OS bring-up path treats `.nxb` as a deterministic directory containing
`manifest.*` + `payload.elf` (see `docs/packaging/nxb.md`). Do not use this tar layout as a contract
for current OS work unless a task explicitly revives it.

A Nexus Bundle (legacy) is a tar archive with the following layout:

```text
/manifest/bundle.toml
/code/<target>/app.bin
/res/**
/perm/entitlements.toml
/sig/*
```

Manifests follow TOML syntax and describe abilities, capabilities, and entry points. Assets and localization resources live under `res/`. The signature directory carries detached metadata for bundle validation.

Example manifest for the launcher:

```toml
[package]
name = "launcher"
version = "0.1.0"

[ability]
entry = "launcher::main"

[permissions]
required = ["windowd.open", "bundle.query"]
```
