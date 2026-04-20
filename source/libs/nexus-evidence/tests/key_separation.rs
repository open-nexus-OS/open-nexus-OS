// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: P5-04 key-separation tests. Locks the resolution
//! priority of [`nexus_evidence::key::from_env_or_dir`]:
//!   1. `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` env → CI label
//!   2. bringup file at `$NEXUS_EVIDENCE_BRINGUP_PRIVKEY` (or
//!      `~/.config/nexus/bringup-key/private.ed25519`) with
//!      mode 0600 → Bringup label
//!   3. otherwise → KeyMaterialMissing
//!
//! Tests run serially (env mutation) by gating on a single mutex.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-04 surface)

use std::sync::Mutex;

use nexus_evidence::{key, EvidenceError, KeyLabel};

mod tempdir;
use tempdir::Tempdir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fixed_seed() -> [u8; 32] {
    // SECURITY: bring-up test keys; deterministic for reproducible CI.
    let mut s = [0u8; 32];
    for (i, b) in s.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(11) ^ 0x33;
    }
    s
}

fn scrub_env() {
    std::env::remove_var("NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64");
    std::env::remove_var("NEXUS_EVIDENCE_BRINGUP_PRIVKEY");
    std::env::remove_var("HOME");
}

#[test]
fn ci_env_takes_precedence() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scrub_env();
    // base64 of 32 zero bytes:
    let b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    std::env::set_var("NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64", b64);

    let (label, _sk) = key::from_env_or_dir().expect("ci env must resolve");
    assert_eq!(label, KeyLabel::Ci);
    scrub_env();
}

#[test]
fn bringup_file_used_when_env_absent() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scrub_env();

    let dir = Tempdir::new("p5_04_bringup_ok");
    let key_path = dir.path().join("private.ed25519");
    let raw = fixed_seed();
    std::fs::write(&key_path, raw).unwrap();
    chmod_0600(&key_path);

    std::env::set_var("NEXUS_EVIDENCE_BRINGUP_PRIVKEY", &key_path);

    let (label, _sk) = key::from_env_or_dir().expect("bringup file must resolve");
    assert_eq!(label, KeyLabel::Bringup);
    scrub_env();
}

#[test]
fn bringup_file_rejected_when_world_readable() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scrub_env();

    let dir = Tempdir::new("p5_04_bringup_perm");
    let key_path = dir.path().join("private.ed25519");
    std::fs::write(&key_path, fixed_seed()).unwrap();
    chmod(&key_path, 0o644);

    std::env::set_var("NEXUS_EVIDENCE_BRINGUP_PRIVKEY", &key_path);
    let err = key::from_env_or_dir().unwrap_err();
    assert!(
        matches!(err, EvidenceError::KeyMaterialPermissions { mode, .. } if mode == 0o644),
        "expected KeyMaterialPermissions(mode=0o644), got {:?}",
        err
    );
    scrub_env();
}

#[test]
fn missing_everywhere_returns_key_material_missing() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scrub_env();
    // Point HOME at an empty tempdir so the default
    // ~/.config/nexus/bringup-key/private.ed25519 doesn't exist.
    let dir = Tempdir::new("p5_04_missing");
    std::env::set_var("HOME", dir.path());

    let err = key::from_env_or_dir().unwrap_err();
    assert!(
        matches!(err, EvidenceError::KeyMaterialMissing),
        "expected KeyMaterialMissing, got {:?}",
        err
    );
    scrub_env();
}

#[cfg(unix)]
fn chmod(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let mut p = std::fs::metadata(path).unwrap().permissions();
    p.set_mode(mode);
    std::fs::set_permissions(path, p).unwrap();
}

#[cfg(unix)]
fn chmod_0600(path: &std::path::Path) {
    chmod(path, 0o600);
}

#[cfg(not(unix))]
fn chmod(_path: &std::path::Path, _mode: u32) {}
#[cfg(not(unix))]
fn chmod_0600(_path: &std::path::Path) {}
