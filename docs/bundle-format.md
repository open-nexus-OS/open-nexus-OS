# Nexus Bundle (`.nxb`) Format

A Nexus Bundle is a tar archive with the following layout:

```
/manifest/bundle.toml
/code/<target>/app.bin
/res/**
/perm/entitlements.toml
/sig/*
```

Manifests follow TOML syntax and describe abilities, capabilities, and entry points. Assets and localization resources live under `res/`. The signature directory carries detached metadata for bundle validation.

Example manifest for the launcher:

```
[package]
name = "launcher"
version = "0.1.0"

[ability]
entry = "launcher::main"

[permissions]
required = ["windowd.open", "bundle.query"]
```
