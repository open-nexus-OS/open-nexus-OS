# Anchor Keys for Keystore

This directory contains public anchors used by `keystored` to verify detached Ed25519 signatures.

- Accepted formats: raw hex (32 bytes, lowercase hex) or PEM (`-----BEGIN PUBLIC KEY-----`).
- Device ID is derived as hex(SHA256(pubkey)[..16]) and used as anchor ID.
- For development, add `*.pub` files here; private keys must not be committed.

Environment overrides:
- Set `NEXUS_ANCHORS_DIR=/abs/path/to/dir` to point `keystored` at a different directory.

Example hex file:
```
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

