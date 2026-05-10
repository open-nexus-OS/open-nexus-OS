// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Ed25519 sign/verify primitives for evidence bundles
//! (P5-03). Defines the on-disk signature byte format
//! (`signature.bin` inside the bundle tar.gz):
//!
//! ```text
//!     magic    : 4 bytes  = b"NXSE"   (Open Nexus Sealed Evidence)
//!     version  : 1 byte   = 0x01      (current; bumps freeze the spec)
//!     label    : 1 byte   = 0x01 Ci | 0x02 Bringup
//!     hash     : 32 bytes = canonical_hash(bundle)
//!     sig      : 64 bytes = Ed25519(privkey, hash)
//!     ────────────────────────────────────────────
//!     total    : 102 bytes
//! ```
//!
//! The signature commits to the canonical hash, NOT to the raw tar
//! bytes — re-packing the same bundle on a different host (different
//! tar implementation, different temp filenames) MUST verify against
//! the same signature, which is what the canonical hash already
//! guarantees (P5-01 § canonical hash).
//!
//! Key model (extended in P5-04):
//!   - `KeyLabel::Ci`      — CI runners; private key from env
//!     (`NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`); public key under
//!     `keys/evidence-ci.pub.ed25519` (committed).
//!   - `KeyLabel::Bringup` — local devs; private key under
//!     `~/.config/nexus/bringup-key/private.ed25519` (chmod 0600);
//!     public key alongside.
//!
//! P5-03 (this cut) adds the byte format, sign/verify primitives,
//! and the 5 tamper classes. Key material loading
//! (`KeyLabel::from_env_or_dir`), CI-vs-bringup precedence, and the
//! secret scanner land in P5-04.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-03 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/sign_verify.rs` (8 tests)

use ed25519_dalek::{
    Signature as DalekSig, Signer, SigningKey as DalekSigning, Verifier,
    VerifyingKey as DalekVerifying,
};

use crate::EvidenceError;

/// Identifier for the key that signed a bundle. Encoded into the
/// signature byte stream so a verifier can refuse a bundle that is
/// signed with the "wrong" key class for its policy
/// (e.g. `--policy=ci` rejects a bundle sealed with `Bringup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyLabel {
    /// CI runner key class. Private key sourced from the
    /// `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` env var; public key
    /// committed under `keys/evidence-ci.pub.ed25519` (P5-04).
    Ci,
    /// Local-developer bring-up key class. Private key under
    /// `~/.config/nexus/bringup-key/private.ed25519` (chmod 0600);
    /// public key alongside (P5-04).
    Bringup,
}

impl KeyLabel {
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            KeyLabel::Ci => 0x01,
            KeyLabel::Bringup => 0x02,
        }
    }

    pub(crate) fn from_byte(b: u8) -> Result<KeyLabel, EvidenceError> {
        match b {
            0x01 => Ok(KeyLabel::Ci),
            0x02 => Ok(KeyLabel::Bringup),
            other => Err(EvidenceError::SignatureMalformed {
                detail: format!("unknown_label_byte 0x{:02x}", other),
            }),
        }
    }

    /// Stable lowercase string form (`"ci"` / `"bringup"`); matches
    /// the `--label=` flag accepted by the CLI and the
    /// `tools/seal-evidence.sh` wrapper.
    pub fn as_str(self) -> &'static str {
        match self {
            KeyLabel::Ci => "ci",
            KeyLabel::Bringup => "bringup",
        }
    }

    /// Inverse of [`Self::as_str`]. Rejects unknown strings with
    /// [`EvidenceError::SignatureMalformed`].
    pub fn parse(s: &str) -> Result<KeyLabel, EvidenceError> {
        match s {
            "ci" => Ok(KeyLabel::Ci),
            "bringup" => Ok(KeyLabel::Bringup),
            other => Err(EvidenceError::SignatureMalformed {
                detail: format!("unknown_label `{}`", other),
            }),
        }
    }
}

/// Magic bytes: `NXSE` = "Open Nexus Sealed Evidence".
const MAGIC: &[u8; 4] = b"NXSE";

/// Current signature wire-format version. Bumping this is a hard
/// break: verifiers MUST reject unknown versions
/// (`UnsupportedSignatureVersion`).
const VERSION: u8 = 0x01;

/// Total fixed length of a `signature.bin` blob.
pub const SIGNATURE_LEN: usize = 4 + 1 + 1 + 32 + 64;

/// In-memory representation of a parsed `signature.bin` blob. The
/// hash is the canonical hash the bundle committed to at sign-time;
/// verifiers re-compute the canonical hash of the (re-read) bundle
/// and reject any mismatch as [`EvidenceError::SignatureMismatch`].
#[derive(Clone, Debug)]
pub struct Signature {
    /// Key class that produced this signature.
    pub label: KeyLabel,
    /// Canonical hash committed at sign-time.
    pub hash: [u8; 32],
    /// Raw 64-byte Ed25519 signature over [`Self::hash`].
    pub sig: [u8; 64],
}

impl Signature {
    /// Encode the signature into its 102-byte on-disk form.
    pub fn to_bytes(&self) -> [u8; SIGNATURE_LEN] {
        let mut out = [0u8; SIGNATURE_LEN];
        out[0..4].copy_from_slice(MAGIC);
        out[4] = VERSION;
        out[5] = self.label.to_byte();
        out[6..38].copy_from_slice(&self.hash);
        out[38..102].copy_from_slice(&self.sig);
        out
    }

    /// Decode a 102-byte blob. Rejects unknown magic, unsupported
    /// version, unknown label, and any length other than
    /// [`SIGNATURE_LEN`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Signature, EvidenceError> {
        if bytes.len() != SIGNATURE_LEN {
            return Err(EvidenceError::SignatureMalformed {
                detail: format!("bad_length got={} want={}", bytes.len(), SIGNATURE_LEN),
            });
        }
        if &bytes[0..4] != MAGIC {
            return Err(EvidenceError::SignatureMalformed {
                detail: format!(
                    "bad_magic got=0x{:02x}{:02x}{:02x}{:02x}",
                    bytes[0], bytes[1], bytes[2], bytes[3]
                ),
            });
        }
        let version = bytes[4];
        if version != VERSION {
            return Err(EvidenceError::UnsupportedSignatureVersion {
                got: version,
                supported: VERSION,
            });
        }
        let label = KeyLabel::from_byte(bytes[5])?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes[6..38]);
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&bytes[38..102]);
        Ok(Signature { label, hash, sig })
    }
}

/// Wrapper around the dalek signing key. Provides only the operations
/// the bundle pipeline needs (sign over a 32-byte hash); key
/// generation and on-disk loading land in P5-04.
#[derive(Clone)]
pub struct SigningKey {
    inner: DalekSigning,
}

impl std::fmt::Debug for SigningKey {
    /// Intentionally redacted: never include private key material in
    /// `Debug` output (it ends up in panic messages, test failures,
    /// and `tracing` records). See SECURITY_STANDARDS.md §"secret
    /// hygiene".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("inner", &"<redacted>")
            .finish()
    }
}

/// Resolve the active signing key + label using the P5-04 priority:
///
/// 1. `NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64` env var → CI label.
/// 2. `~/.config/nexus/bringup-key/private.ed25519` (chmod 0600) →
///    Bringup label.
/// 3. otherwise → [`EvidenceError::KeyMaterialMissing`].
///
/// Override the bringup file path with `NEXUS_EVIDENCE_BRINGUP_PRIVKEY`
/// (used by tests + `tools/seal-evidence.sh`); override the home
/// directory with `HOME` (already done by `std::env::home_dir`'s
/// successor [`home::home_dir`] — we avoid that dep and use `HOME`
/// directly).
///
/// Permission check: if the bringup file is selected and its mode
/// is not exactly `0600`, returns
/// [`EvidenceError::KeyMaterialPermissions`] with the offending
/// mode. CI env material is in-memory only and bypasses the
/// permission check by construction.
pub fn from_env_or_dir() -> Result<(KeyLabel, SigningKey), EvidenceError> {
    if let Ok(b64) = std::env::var("NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64") {
        let trimmed = b64.trim();
        if !trimmed.is_empty() {
            let raw = decode_seed(trimmed.as_bytes())?;
            return Ok((KeyLabel::Ci, SigningKey::from_seed(raw)));
        }
    }
    let bringup_path = match std::env::var("NEXUS_EVIDENCE_BRINGUP_PRIVKEY") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => {
            let home = std::env::var("HOME").map_err(|_| EvidenceError::KeyMaterialMissing)?;
            std::path::PathBuf::from(home)
                .join(".config")
                .join("nexus")
                .join("bringup-key")
                .join("private.ed25519")
        }
    };
    if !bringup_path.exists() {
        return Err(EvidenceError::KeyMaterialMissing);
    }
    check_perm_0600(&bringup_path)?;
    let bytes = std::fs::read(&bringup_path).map_err(|_| EvidenceError::KeyMaterialMissing)?;
    let raw = decode_seed(&bytes)?;
    Ok((KeyLabel::Bringup, SigningKey::from_seed(raw)))
}

#[cfg(unix)]
fn check_perm_0600(path: &std::path::Path) -> Result<(), EvidenceError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|_| EvidenceError::KeyMaterialMissing)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(EvidenceError::KeyMaterialPermissions {
            path: path.display().to_string(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_perm_0600(_path: &std::path::Path) -> Result<(), EvidenceError> {
    Ok(())
}

/// Parse a 32-byte Ed25519 seed from raw|hex|base64 input bytes.
/// Mirrors the CLI helper of the same name; lifted here so
/// `from_env_or_dir` stays self-contained.
fn decode_seed(bytes: &[u8]) -> Result<[u8; 32], EvidenceError> {
    if bytes.len() == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(bytes);
        return Ok(out);
    }
    let s = std::str::from_utf8(bytes).map_err(|_| EvidenceError::KeyMaterialMissing)?;
    let trimmed: String = s.split_whitespace().collect();
    if trimmed.len() == 64 {
        if let Ok(raw) = hex_decode(&trimmed) {
            if raw.len() == 32 {
                let mut out = [0u8; 32];
                out.copy_from_slice(&raw);
                return Ok(out);
            }
        }
    }
    if let Some(raw) = base64_decode(&trimmed) {
        if raw.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&raw);
            return Ok(out);
        }
    }
    Err(EvidenceError::KeyMaterialMissing)
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    // NOTE: `usize::is_multiple_of` is still unstable on the workspace
    // pinned toolchain (nightly-2025-01-15); revisit once the pin moves.
    // The `unknown_lints` allow is needed because `manual_is_multiple_of`
    // landed in clippy *after* nightly-2025-01-15: stable/newer clippy
    // (used by `just lint`) emits the lint, the pinned older clippy
    // (used by `scripts/fmt-clippy-deny.sh` / `make build`) does not
    // know it yet and would otherwise fail under `-D warnings`.
    #[allow(unknown_lints, clippy::manual_is_multiple_of)]
    if s.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = nib(bytes[i])?;
        let lo = nib(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn nib(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    // See note in `hex_decode`: `is_multiple_of` is unstable on the
    // workspace toolchain pin and the lint name is unknown there.
    #[allow(unknown_lints, clippy::manual_is_multiple_of)]
    if s.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let mut group = [0u8; 4];
        let mut pad = 0;
        for j in 0..4 {
            let b = bytes[i + j];
            group[j] = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    pad += 1;
                    0
                }
                _ => return None,
            };
        }
        let chunk = (u32::from(group[0]) << 18)
            | (u32::from(group[1]) << 12)
            | (u32::from(group[2]) << 6)
            | u32::from(group[3]);
        out.push((chunk >> 16) as u8);
        if pad < 2 {
            out.push((chunk >> 8) as u8);
        }
        if pad < 1 {
            out.push(chunk as u8);
        }
        i += 4;
    }
    Some(out)
}

impl SigningKey {
    /// Construct from raw 32-byte secret-key seed material.
    pub fn from_seed(seed: [u8; 32]) -> SigningKey {
        SigningKey {
            inner: DalekSigning::from_bytes(&seed),
        }
    }

    /// Derive the matching public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey {
            inner: self.inner.verifying_key(),
        }
    }

    /// Sign a 32-byte canonical hash, producing a [`Signature`]
    /// blob carrying the supplied `label`.
    pub fn sign_hash(&self, hash: [u8; 32], label: KeyLabel) -> Signature {
        let dalek_sig: DalekSig = self.inner.sign(&hash);
        Signature {
            label,
            hash,
            sig: dalek_sig.to_bytes(),
        }
    }
}

/// Wrapper around the dalek verifying key. Used by `SealedBundle::
/// verify` (P5-03) and by `tools/verify-evidence.sh` (P5-04).
#[derive(Clone, Debug)]
pub struct VerifyingKey {
    inner: DalekVerifying,
}

impl VerifyingKey {
    /// Construct from a 32-byte public-key blob (the on-disk
    /// `*.pub.ed25519` format).
    pub fn from_bytes(bytes: [u8; 32]) -> Result<VerifyingKey, EvidenceError> {
        DalekVerifying::from_bytes(&bytes)
            .map(|inner| VerifyingKey { inner })
            .map_err(|e| EvidenceError::SignatureMalformed {
                detail: format!("bad_pubkey {}", e),
            })
    }

    /// Encode back to 32 bytes (round-trip with [`Self::from_bytes`]).
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Verify that `sig` is a valid signature over the canonical
    /// hash that `expected_hash` evaluated to. Returns
    /// [`EvidenceError::SignatureMismatch`] on either a hash
    /// mismatch (the bundle was tampered after signing) or a
    /// signature mismatch (the bundle was signed with a different
    /// key than the verifier holds).
    pub fn verify(&self, sig: &Signature, expected_hash: &[u8; 32]) -> Result<(), EvidenceError> {
        if sig.hash != *expected_hash {
            return Err(EvidenceError::SignatureMismatch {
                detail: "canonical_hash_changed".to_string(),
            });
        }
        let dalek_sig = DalekSig::from_bytes(&sig.sig);
        self.inner
            .verify(&sig.hash, &dalek_sig)
            .map_err(|_| EvidenceError::SignatureMismatch {
                detail: "ed25519_verify_failed".to_string(),
            })
    }
}
