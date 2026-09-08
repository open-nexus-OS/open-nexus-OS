# Sandboxing v1 Security Notes

## Scope Boundary

Sandboxing v1 is a userspace confinement floor. It does not claim kernel-enforced namespace or syscall containment.

- Kernel remains untouched.
- Confinement depends on spawn-time capability distribution (`execd/init` authority).
- App subjects must not receive direct `packagefsd` or `statefsd` capabilities.

## Network boundary (TASK-0043)

Outbound policy is enforced at netstackd's facade (`docs/security/network-egress.md`)
and inbound binds at the same seam (TASK-0052). A subject that holds
`device.mmio.net` drives the NIC itself and bypasses both — the grant is the
boundary, not the facade: never grant it to a sandboxed app subject.

## v1 Enforcement Floor

- Namespace path handling is canonicalized and rejects traversal (`..`) deterministically.
- CapFd token checks are fail-closed for integrity, replay, subject binding, expiry, and rights subset.
- Reject behavior is deterministic for unauthorized namespace paths and rights mismatches.

## Required Reject Proofs

Run these host proofs:

```bash
cd /home/jenning/open-nexus-OS && cargo test -p vfsd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p nexus-vfs -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture
```

Required test names:

- `test_reject_path_traversal`
- `test_reject_unauthorized_namespace_path`
- `test_reject_forged_capfd`
- `test_reject_replayed_capfd`
- `test_reject_capfd_rights_mismatch`
- `test_reject_direct_fs_cap_bypass_at_spawn_boundary`

## OS-Gated Marker Contract

Only stable labels are allowed:

- `vfsd: namespace ready`
- `vfsd: capfd grant ok`
- `vfsd: access denied`
- `SELFTEST: sandbox deny ok`
- `SELFTEST: capfd read ok`
