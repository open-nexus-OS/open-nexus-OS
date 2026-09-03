// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: NXSV — Nexus System Volume descriptor (RFC-0089 §12.2,
//! TASK-0321). One 512-byte sector at system-partition sector 0, the SAME
//! shape as the NXBD boot descriptor (§5) with its own magic: signed at
//! build time by `nx image` with the OS-image key, written VERBATIM by
//! `updated` (NXSV-last — a volume without a valid NXSV is invalid by
//! definition), verified by `bundlemgrd` — the boot image, never the
//! loader — against the baked OS keys, the persisted rollback floor and
//! the MEASURED boot image (`boot_image_sha256` pairs system-X with
//! boot-X). Bounded, deterministic, no_std, panic-free on untrusted bytes.
//! OWNERS: @reliability @security @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (roundtrip, tamper/wrong key,
//!   reserved-byte fail-closed, zeroed sector invalid, layout golden)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use super::{FmtError, SECTOR};

/// Signed span: bytes [0..448); the Ed25519 signature fills [448..512).
pub const SIGNED_LEN: usize = 448;
pub const MAGIC: &[u8; 8] = b"NXSV0001";
pub const VERSION: u16 = 1;

/// Decoded descriptor fields (signature carried separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nxsv {
    /// Same anti-downgrade line as the paired boot image (§10).
    pub rollback_index: u32,
    /// pkgimg bytes from sector 8.
    pub volume_size: u64,
    /// sha256 over the pkgimg bytes.
    pub volume_sha256: [u8; 32],
    /// ASCII build id, NUL-padded — equals the paired NXBD build id.
    pub build_id: [u8; 32],
    /// First 8 bytes of the signer's public key (anchor lookup hint).
    pub pubkey_id: [u8; 8],
    /// Pairing: the NXBD `image_sha256` this volume belongs to.
    pub boot_image_sha256: [u8; 32],
    /// pkgimg superblock + index bytes (≤ 256 KiB; the kind-6 payload).
    pub index_len: u32,
    /// sha256 over the first `index_len` payload bytes.
    pub index_sha256: [u8; 32],
}

impl Nxsv {
    /// Printable build id (trailing NULs stripped; lossy-safe ASCII).
    pub fn build_id_str(&self) -> &str {
        let end = self.build_id.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.build_id[..end]).unwrap_or("?")
    }
}

/// Encodes descriptor + signature into the 512-byte sector.
pub fn encode(d: &Nxsv, signature: &[u8; 64]) -> [u8; SECTOR] {
    let mut out = [0u8; SECTOR];
    out[0..8].copy_from_slice(MAGIC);
    out[8..10].copy_from_slice(&VERSION.to_le_bytes());
    // [10..12) flags: reserved, zero.
    out[12..16].copy_from_slice(&d.rollback_index.to_le_bytes());
    out[16..24].copy_from_slice(&d.volume_size.to_le_bytes());
    out[24..56].copy_from_slice(&d.volume_sha256);
    out[56..88].copy_from_slice(&d.build_id);
    // [88..96) reserved (NXBD's load_addr slot) — zero.
    out[96..104].copy_from_slice(&d.pubkey_id);
    out[104..136].copy_from_slice(&d.boot_image_sha256);
    out[136..140].copy_from_slice(&d.index_len.to_le_bytes());
    out[140..172].copy_from_slice(&d.index_sha256);
    // [172..448) reserved — zero.
    out[448..512].copy_from_slice(signature);
    out
}

/// Encodes the UNSIGNED prefix (for signing).
pub fn encode_unsigned(d: &Nxsv) -> [u8; SIGNED_LEN] {
    let full = encode(d, &[0u8; 64]);
    let mut out = [0u8; SIGNED_LEN];
    out.copy_from_slice(&full[..SIGNED_LEN]);
    out
}

/// Bounded decode: magic/version/flags/reserved checks, no crypto. A
/// zeroed sector (factory-empty volume) is `Malformed` by design.
pub fn decode(bytes: &[u8]) -> Result<(Nxsv, [u8; 64]), FmtError> {
    if bytes.len() != SECTOR || &bytes[0..8] != MAGIC {
        return Err(FmtError::Malformed);
    }
    if u16::from_le_bytes([bytes[8], bytes[9]]) != VERSION {
        return Err(FmtError::Malformed);
    }
    if bytes[10] != 0 || bytes[11] != 0 {
        return Err(FmtError::Reserved);
    }
    if bytes[88..96].iter().any(|&b| b != 0) || bytes[172..448].iter().any(|&b| b != 0) {
        return Err(FmtError::Reserved);
    }
    let mut d = Nxsv {
        rollback_index: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        volume_size: u64_at(bytes, 16),
        volume_sha256: [0u8; 32],
        build_id: [0u8; 32],
        pubkey_id: [0u8; 8],
        boot_image_sha256: [0u8; 32],
        index_len: u32::from_le_bytes([bytes[136], bytes[137], bytes[138], bytes[139]]),
        index_sha256: [0u8; 32],
    };
    d.volume_sha256.copy_from_slice(&bytes[24..56]);
    d.build_id.copy_from_slice(&bytes[56..88]);
    d.pubkey_id.copy_from_slice(&bytes[96..104]);
    d.boot_image_sha256.copy_from_slice(&bytes[104..136]);
    d.index_sha256.copy_from_slice(&bytes[140..172]);
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&bytes[448..512]);
    Ok((d, sig))
}

/// Verifies the descriptor signature against `pubkey` (pure, no RNG).
pub fn verify(sector: &[u8], pubkey: &[u8; 32]) -> Result<Nxsv, FmtError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let (d, sig) = decode(sector)?;
    let key = VerifyingKey::from_bytes(pubkey).map_err(|_| FmtError::Signature)?;
    key.verify(&sector[..SIGNED_LEN], &Signature::from_bytes(&sig))
        .map_err(|_| FmtError::Signature)?;
    Ok(d)
}

/// Signs a descriptor with an Ed25519 seed (host tooling: `nx image`).
#[cfg(feature = "sign")]
pub fn sign(d: &Nxsv, seed: &[u8; 32]) -> [u8; SECTOR] {
    use ed25519_dalek::{Signer, SigningKey};
    let key = SigningKey::from_bytes(seed);
    let unsigned = encode_unsigned(d);
    let sig = key.sign(&unsigned);
    encode(d, &sig.to_bytes())
}

fn u64_at(bytes: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[off..off + 8]);
    u64::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nxbd;

    const SEED: [u8; 32] = [11u8; 32];

    fn sample() -> Nxsv {
        let (_pk, id) = nxbd::pubkey_id_for_seed(&SEED);
        Nxsv {
            rollback_index: 7,
            volume_size: 3_000_000,
            volume_sha256: [0xAB; 32],
            build_id: nxbd::Nxbd::build_id_from("dev-2026-09-03"),
            pubkey_id: id,
            boot_image_sha256: [0xCD; 32],
            index_len: 4096,
            index_sha256: [0xEF; 32],
        }
    }

    #[test]
    fn nxsv_sign_verify_roundtrip_and_layout_golden() {
        let d = sample();
        let sector = sign(&d, &SEED);
        let (pk, _) = nxbd::pubkey_id_for_seed(&SEED);
        let back = verify(&sector, &pk).expect("verify");
        assert_eq!(back, d);
        assert_eq!(back.build_id_str(), "dev-2026-09-03");
        // Layout freeze (RFC-0089 §12.2): field positions.
        assert_eq!(&sector[0..8], MAGIC);
        assert_eq!(sector[12], 7);
        assert_eq!(&sector[104..136], &[0xCD; 32]);
        assert_eq!(u32::from_le_bytes([sector[136], sector[137], sector[138], sector[139]]), 4096);
        assert_eq!(&sector[140..172], &[0xEF; 32]);
        assert!(sector[88..96].iter().all(|&b| b == 0));
        assert!(sector[172..448].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_reject_nxsv_sig_and_wrong_key() {
        let d = sample();
        let mut sector = sign(&d, &SEED);
        let (pk, _) = nxbd::pubkey_id_for_seed(&SEED);
        sector[110] ^= 1; // pairing digest tamper
        assert_eq!(verify(&sector, &pk), Err(FmtError::Signature));
        sector[110] ^= 1;
        sector[460] ^= 1; // signature tamper
        assert_eq!(verify(&sector, &pk), Err(FmtError::Signature));
        sector[460] ^= 1;
        let (other_pk, _) = nxbd::pubkey_id_for_seed(&[12u8; 32]);
        assert_eq!(verify(&sector, &other_pk), Err(FmtError::Signature));
    }

    #[test]
    fn test_reject_nxsv_reserved_and_magic() {
        let d = sample();
        let sector = sign(&d, &SEED);
        let (pk, _) = nxbd::pubkey_id_for_seed(&SEED);
        let mut dirty = sector;
        dirty[90] = 1;
        assert_eq!(verify(&dirty, &pk), Err(FmtError::Reserved));
        let mut dirty = sector;
        dirty[300] = 1;
        assert_eq!(verify(&dirty, &pk), Err(FmtError::Reserved));
        // An NXBD sector is NOT a valid NXSV (own magic).
        let mut wrong = sector;
        wrong[0..8].copy_from_slice(nxbd::MAGIC);
        assert_eq!(decode(&wrong).map(|_| ()), Err(FmtError::Malformed));
        assert_eq!(decode(&[0u8; 512]).map(|_| ()), Err(FmtError::Malformed));
    }
}
