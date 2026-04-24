extern crate alloc;

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use sha2::{Digest, Sha256};

pub const RIGHT_READ: u8 = 0x01;
pub const RIGHT_WRITE: u8 = 0x02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxError {
    InvalidPath,
    Traversal,
    OutOfNamespace,
    Integrity,
    Replay,
    Rights,
    Subject,
    Expired,
}

#[derive(Debug, Clone)]
pub struct NamespaceView {
    roots: Vec<String>,
}

impl NamespaceView {
    pub fn new(roots: Vec<String>) -> Self {
        Self { roots }
    }

    pub fn canonical_path(&self, path: &str) -> core::result::Result<String, SandboxError> {
        canonicalize_path(path)
    }

    pub fn assert_allowed(&self, path: &str) -> core::result::Result<String, SandboxError> {
        let canonical = self.canonical_path(path)?;
        if self.roots.iter().any(|root| canonical.starts_with(root)) {
            Ok(canonical)
        } else {
            Err(SandboxError::OutOfNamespace)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapFdToken {
    pub subject_id: u64,
    pub canonical_path: String,
    pub rights: u8,
    pub nonce: u64,
    pub expires_at: u64,
    pub mac: [u8; 32],
}

impl CapFdToken {
    pub fn mint(
        mac_key: &[u8],
        subject_id: u64,
        canonical_path: String,
        rights: u8,
        nonce: u64,
        expires_at: u64,
    ) -> Self {
        let mac = compute_mac(mac_key, subject_id, &canonical_path, rights, nonce, expires_at);
        Self { subject_id, canonical_path, rights, nonce, expires_at, mac }
    }
}

#[derive(Debug, Default)]
pub struct ReplayGuard {
    seen: BTreeSet<u64>,
}

impl ReplayGuard {
    pub fn verify(
        &mut self,
        mac_key: &[u8],
        token: &CapFdToken,
        expected_subject_id: u64,
        required_rights: u8,
        now: u64,
    ) -> core::result::Result<(), SandboxError> {
        if token.subject_id != expected_subject_id {
            return Err(SandboxError::Subject);
        }
        if token.expires_at < now {
            return Err(SandboxError::Expired);
        }
        if required_rights != 0 && (token.rights & required_rights) != required_rights {
            return Err(SandboxError::Rights);
        }
        let expected = compute_mac(
            mac_key,
            token.subject_id,
            &token.canonical_path,
            token.rights,
            token.nonce,
            token.expires_at,
        );
        if expected != token.mac {
            return Err(SandboxError::Integrity);
        }
        if !self.seen.insert(token.nonce) {
            return Err(SandboxError::Replay);
        }
        Ok(())
    }
}

fn canonicalize_path(path: &str) -> core::result::Result<String, SandboxError> {
    let (scheme, rest) = path.split_once(":/").ok_or(SandboxError::InvalidPath)?;
    if scheme.is_empty() {
        return Err(SandboxError::InvalidPath);
    }
    if rest.is_empty() {
        return Err(SandboxError::InvalidPath);
    }
    let mut out_segments: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(SandboxError::Traversal);
        }
        out_segments.push(seg);
    }
    if out_segments.is_empty() {
        return Err(SandboxError::InvalidPath);
    }
    Ok(format_canonical(scheme, &out_segments))
}

fn format_canonical(scheme: &str, segments: &[&str]) -> String {
    let mut out = scheme.to_string();
    out.push_str(":/");
    for (idx, seg) in segments.iter().enumerate() {
        if idx != 0 {
            out.push('/');
        }
        out.push_str(seg);
    }
    out
}

fn compute_mac(
    key: &[u8],
    subject_id: u64,
    canonical_path: &str,
    rights: u8,
    nonce: u64,
    expires_at: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(subject_id.to_le_bytes());
    hasher.update(canonical_path.as_bytes());
    hasher.update([rights]);
    hasher.update(nonce.to_le_bytes());
    hasher.update(expires_at.to_le_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        CapFdToken, NamespaceView, ReplayGuard, SandboxError, RIGHT_READ, RIGHT_WRITE,
    };

    const KEY: &[u8] = b"v1-sandbox-mac-key";

    #[test]
    fn test_reject_path_traversal() {
        let ns = NamespaceView::new(vec!["pkg:/system/".to_string()]);
        let err = ns.assert_allowed("pkg:/system/../secrets.txt").expect_err("must reject");
        assert_eq!(err, SandboxError::Traversal);
    }

    #[test]
    fn test_reject_unauthorized_namespace_path() {
        let ns = NamespaceView::new(vec!["pkg:/system/".to_string()]);
        let err = ns.assert_allowed("pkg:/other/config.toml").expect_err("must reject");
        assert_eq!(err, SandboxError::OutOfNamespace);
    }

    #[test]
    fn test_reject_forged_capfd() {
        let mut guard = ReplayGuard::default();
        let mut token =
            CapFdToken::mint(KEY, 7, "pkg:/system/build.prop".to_string(), RIGHT_READ, 1, 10_000);
        token.mac[0] ^= 0xFF;
        let err = guard
            .verify(KEY, &token, 7, RIGHT_READ, 5_000)
            .expect_err("must reject forged token");
        assert_eq!(err, SandboxError::Integrity);
    }

    #[test]
    fn test_reject_replayed_capfd() {
        let mut guard = ReplayGuard::default();
        let token =
            CapFdToken::mint(KEY, 7, "pkg:/system/build.prop".to_string(), RIGHT_READ, 42, 10_000);
        assert!(guard.verify(KEY, &token, 7, RIGHT_READ, 5_000).is_ok());
        let err = guard
            .verify(KEY, &token, 7, RIGHT_READ, 5_000)
            .expect_err("must reject replay");
        assert_eq!(err, SandboxError::Replay);
    }

    #[test]
    fn test_reject_capfd_rights_mismatch() {
        let mut guard = ReplayGuard::default();
        let token =
            CapFdToken::mint(KEY, 7, "pkg:/system/build.prop".to_string(), RIGHT_READ, 77, 10_000);
        let err = guard
            .verify(KEY, &token, 7, RIGHT_WRITE, 5_000)
            .expect_err("must reject rights mismatch");
        assert_eq!(err, SandboxError::Rights);
    }
}
