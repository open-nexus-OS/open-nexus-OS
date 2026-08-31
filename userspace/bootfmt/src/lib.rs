// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::all)]

//! CONTEXT: Fixed-layout boot-format codecs (RFC-0089): `nxbd` — the
//! 512-byte SIGNED boot descriptor at slot-partition sector 0 (§5, signed
//! at build time by `nx image`, written verbatim by `updated`, verified by
//! `nxboot` BEFORE any OS code runs) — and `bsb` — the 512-byte
//! boot-selection double block (§6; ADR-0058 write matrix). Both are
//! bounded, deterministic, no_std, panic-free on untrusted bytes; the
//! loader links the verify half only (pure Ed25519, no RNG).
//! OWNERS: @reliability @security @runtime
//! PUBLIC API: nxbd::{Nxbd, encode/decode/sign/verify}, bsb::{Bsb,
//!   encode/decode/pick, factory}, handoff::{Handoff, encode_page/decode}
//!   (the ADR-0059 measured-boot page, written by nxboot, probed by neuron)
//! TEST_COVERAGE: unit tests below (goldens, roundtrips, tamper, torn
//!   block, pick rule)
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md,
//!   docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

extern crate alloc;

/// Shared 512-byte sector size for both formats.
pub const SECTOR: usize = 512;

/// Stable decode-reject reasons (marker/audit vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtError {
    /// Wrong length, magic or version.
    Malformed,
    /// Reserved bytes not zero (fail closed on unknown extensions).
    Reserved,
    /// CRC mismatch (bsb) — torn or corrupt block.
    Crc,
    /// Ed25519 signature invalid (nxbd).
    Signature,
}

pub mod handoff;

/// crc32 (IEEE, table-free) shared by the BSB block and the measured-boot
/// handoff page — same polynomial as GPT.
pub(crate) fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub mod nxbd {
    //! NXBD — Nexus Boot Descriptor (RFC-0089 §5, normative layout).

    use super::{FmtError, SECTOR};

    /// Signed span: bytes [0..448); the Ed25519 signature fills [448..512).
    pub const SIGNED_LEN: usize = 448;
    pub const MAGIC: &[u8; 8] = b"NXBD0001";
    pub const VERSION: u16 = 1;

    /// Decoded descriptor fields (signature carried separately).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Nxbd {
        pub rollback_index: u32,
        pub image_size: u64,
        pub image_sha256: [u8; 32],
        /// ASCII build id, NUL-padded (printed on uart at boot).
        pub build_id: [u8; 32],
        pub load_addr: u64,
        /// First 8 bytes of the signer's public key (anchor lookup hint).
        pub pubkey_id: [u8; 8],
    }

    impl Nxbd {
        /// Build-id helper: ASCII, truncated/NUL-padded to 32.
        pub fn build_id_from(label: &str) -> [u8; 32] {
            let mut out = [0u8; 32];
            for (dst, src) in out.iter_mut().zip(label.bytes()) {
                *dst = if src.is_ascii_graphic() { src } else { b'_' };
            }
            out
        }

        /// Printable build id (trailing NULs stripped; lossy-safe ASCII).
        pub fn build_id_str(&self) -> &str {
            let end = self.build_id.iter().position(|&b| b == 0).unwrap_or(32);
            core::str::from_utf8(&self.build_id[..end]).unwrap_or("?")
        }
    }

    /// Encodes descriptor + signature into the 512-byte sector.
    pub fn encode(d: &Nxbd, signature: &[u8; 64]) -> [u8; SECTOR] {
        let mut out = [0u8; SECTOR];
        out[0..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&VERSION.to_le_bytes());
        // [10..12) flags: reserved, zero.
        out[12..16].copy_from_slice(&d.rollback_index.to_le_bytes());
        out[16..24].copy_from_slice(&d.image_size.to_le_bytes());
        out[24..56].copy_from_slice(&d.image_sha256);
        out[56..88].copy_from_slice(&d.build_id);
        out[88..96].copy_from_slice(&d.load_addr.to_le_bytes());
        out[96..104].copy_from_slice(&d.pubkey_id);
        out[448..512].copy_from_slice(signature);
        out
    }

    /// Encodes the UNSIGNED prefix (for signing).
    pub fn encode_unsigned(d: &Nxbd) -> [u8; SIGNED_LEN] {
        let full = encode(d, &[0u8; 64]);
        let mut out = [0u8; SIGNED_LEN];
        out.copy_from_slice(&full[..SIGNED_LEN]);
        out
    }

    /// Bounded decode: magic/version/flags/reserved checks, no crypto.
    /// A zeroed sector (factory-empty slot) is `Malformed` by design —
    /// an empty slot is INVALID, never bootable.
    pub fn decode(bytes: &[u8]) -> Result<(Nxbd, [u8; 64]), FmtError> {
        if bytes.len() != SECTOR || &bytes[0..8] != MAGIC {
            return Err(FmtError::Malformed);
        }
        if u16::from_le_bytes([bytes[8], bytes[9]]) != VERSION {
            return Err(FmtError::Malformed);
        }
        if bytes[10] != 0 || bytes[11] != 0 {
            return Err(FmtError::Reserved);
        }
        if bytes[104..448].iter().any(|&b| b != 0) {
            return Err(FmtError::Reserved);
        }
        let mut d = Nxbd {
            rollback_index: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            image_size: u64_at(bytes, 16),
            image_sha256: [0u8; 32],
            build_id: [0u8; 32],
            load_addr: u64_at(bytes, 88),
            pubkey_id: [0u8; 8],
        };
        d.image_sha256.copy_from_slice(&bytes[24..56]);
        d.build_id.copy_from_slice(&bytes[56..88]);
        d.pubkey_id.copy_from_slice(&bytes[96..104]);
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&bytes[448..512]);
        Ok((d, sig))
    }

    /// Verifies the descriptor signature against `pubkey` (pure, no RNG —
    /// the nxboot trust root links exactly this).
    pub fn verify(sector: &[u8], pubkey: &[u8; 32]) -> Result<Nxbd, FmtError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let (d, sig) = decode(sector)?;
        let key = VerifyingKey::from_bytes(pubkey).map_err(|_| FmtError::Signature)?;
        key.verify(&sector[..SIGNED_LEN], &Signature::from_bytes(&sig))
            .map_err(|_| FmtError::Signature)?;
        Ok(d)
    }

    /// Signs a descriptor with an Ed25519 seed (host tooling: `nx image`).
    #[cfg(feature = "sign")]
    pub fn sign(d: &Nxbd, seed: &[u8; 32]) -> [u8; SECTOR] {
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(seed);
        let unsigned = encode_unsigned(d);
        let sig = key.sign(&unsigned);
        encode(d, &sig.to_bytes())
    }

    /// First 8 bytes of the verifying key for a seed (pubkey_id helper).
    #[cfg(feature = "sign")]
    pub fn pubkey_id_for_seed(seed: &[u8; 32]) -> ([u8; 32], [u8; 8]) {
        use ed25519_dalek::SigningKey;
        let pk = SigningKey::from_bytes(seed).verifying_key().to_bytes();
        let mut id = [0u8; 8];
        id.copy_from_slice(&pk[..8]);
        (pk, id)
    }

    fn u64_at(bytes: &[u8], off: usize) -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&bytes[off..off + 8]);
        u64::from_le_bytes(b)
    }
}

pub mod bsb {
    //! BSB — Boot Selection Block (RFC-0089 §6; ADR-0058 write matrix).

    use super::{FmtError, SECTOR};

    pub const MAGIC: &[u8; 8] = b"NXBSB1\0\0";
    pub const VERSION: u16 = 1;
    const CRC_OFF: usize = 508;

    /// Slot encoding on the wire: 0 = a, 1 = b, 0xFF = none (next only).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Slot {
        A,
        B,
    }

    impl Slot {
        pub fn other(self) -> Self {
            match self {
                Slot::A => Slot::B,
                Slot::B => Slot::A,
            }
        }
        fn to_wire(self) -> u8 {
            match self {
                Slot::A => 0,
                Slot::B => 1,
            }
        }
        fn from_wire(byte: u8) -> Result<Self, FmtError> {
            match byte {
                0 => Ok(Slot::A),
                1 => Ok(Slot::B),
                _ => Err(FmtError::Malformed),
            }
        }
    }

    /// Decoded selection state (one block of the double pair).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Bsb {
        /// Monotonic write sequence — the reader picks the higher one.
        pub seq: u64,
        pub active_slot: Slot,
        pub next_slot: Option<Slot>,
        pub tries_left: u8,
        /// bit0 = health committed.
        pub health_committed: bool,
        /// RFC-0087 §4 target projection (opaque to the loader).
        pub boot_target: u8,
        /// Anti-downgrade floor (RFC-0089 §10).
        pub rollback_min_index: u32,
    }

    impl Bsb {
        /// Factory block (`nx image build`): seq 1, active A, committed.
        /// `rollback_min_index` is the FLOOR the factory ships at — it must
        /// equal the shipped image's index, otherwise the anti-downgrade
        /// gate is vacuous on a fresh device (index 0 vs floor 0 is not a
        /// downgrade, so the very first update could be a rollback).
        pub fn factory_with_floor(rollback_min_index: u32) -> Self {
            Self { rollback_min_index, ..Self::factory() }
        }

        /// Factory block with a zero floor (tests/legacy callers).
        pub fn factory() -> Self {
            Self {
                seq: 1,
                active_slot: Slot::A,
                next_slot: None,
                tries_left: 0,
                health_committed: true,
                boot_target: 0,
                rollback_min_index: 0,
            }
        }
    }

    /// Encodes one 512-byte block (CRC32 over bytes [0..508)).
    pub fn encode(b: &Bsb) -> [u8; SECTOR] {
        let mut out = [0u8; SECTOR];
        out[0..8].copy_from_slice(MAGIC);
        out[8..16].copy_from_slice(&b.seq.to_le_bytes());
        out[16..18].copy_from_slice(&VERSION.to_le_bytes());
        out[18] = b.active_slot.to_wire();
        out[19] = b.next_slot.map(Slot::to_wire).unwrap_or(0xFF);
        out[20] = b.tries_left;
        out[21] = u8::from(b.health_committed);
        out[22] = b.boot_target;
        // [23] reserved.
        out[24..28].copy_from_slice(&b.rollback_min_index.to_le_bytes());
        let crc = crc32(&out[..CRC_OFF]);
        out[CRC_OFF..SECTOR].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Bounded decode of one block: magic + version + CRC + field checks.
    pub fn decode(bytes: &[u8]) -> Result<Bsb, FmtError> {
        if bytes.len() != SECTOR || &bytes[0..8] != MAGIC {
            return Err(FmtError::Malformed);
        }
        let stored = u32::from_le_bytes([bytes[508], bytes[509], bytes[510], bytes[511]]);
        if crc32(&bytes[..CRC_OFF]) != stored {
            return Err(FmtError::Crc);
        }
        if u16::from_le_bytes([bytes[16], bytes[17]]) != VERSION {
            return Err(FmtError::Malformed);
        }
        let next_slot = match bytes[19] {
            0xFF => None,
            raw => Some(Slot::from_wire(raw)?),
        };
        let mut seq = [0u8; 8];
        seq.copy_from_slice(&bytes[8..16]);
        Ok(Bsb {
            seq: u64::from_le_bytes(seq),
            active_slot: Slot::from_wire(bytes[18])?,
            next_slot,
            tries_left: bytes[20],
            health_committed: bytes[21] & 1 == 1,
            boot_target: bytes[22],
            rollback_min_index: u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        })
    }

    /// The reader rule (RFC-0089 §6): of the two blocks, take the VALID
    /// one with the higher seq. Returns the block index (0/1) so a writer
    /// knows which block to overwrite next (always the OTHER one).
    pub fn pick(block0: &[u8], block1: &[u8]) -> Option<(Bsb, usize)> {
        match (decode(block0), decode(block1)) {
            (Ok(a), Ok(b)) => {
                if b.seq > a.seq {
                    Some((b, 1))
                } else {
                    Some((a, 0))
                }
            }
            (Ok(a), Err(_)) => Some((a, 0)),
            (Err(_), Ok(b)) => Some((b, 1)),
            (Err(_), Err(_)) => None,
        }
    }

    /// crc32 (IEEE) — see crate-level `crc32_ieee` (shared with `handoff`).
    fn crc32(data: &[u8]) -> u32 {
        super::crc32_ieee(data)
    }
}

#[cfg(test)]
mod tests {
    use super::bsb::{self, Bsb, Slot};
    use super::nxbd::{self, Nxbd};
    use super::FmtError;

    const SEED: [u8; 32] = [11u8; 32];

    fn sample_nxbd() -> Nxbd {
        let (_pk, id) = nxbd::pubkey_id_for_seed(&SEED);
        Nxbd {
            rollback_index: 7,
            image_size: 19_000_000,
            image_sha256: [0xAB; 32],
            build_id: Nxbd::build_id_from("dev-2026-08-25"),
            load_addr: 0x8020_0000,
            pubkey_id: id,
        }
    }

    #[test]
    fn nxbd_sign_verify_roundtrip() {
        let d = sample_nxbd();
        let sector = nxbd::sign(&d, &SEED);
        let (pk, _) = nxbd::pubkey_id_for_seed(&SEED);
        let back = nxbd::verify(&sector, &pk).expect("verify");
        assert_eq!(back, d);
        assert_eq!(back.build_id_str(), "dev-2026-08-25");
    }

    #[test]
    fn nxbd_rejects_tamper_and_wrong_key() {
        let d = sample_nxbd();
        let mut sector = nxbd::sign(&d, &SEED);
        let (pk, _) = nxbd::pubkey_id_for_seed(&SEED);
        // Payload tamper (rollback index): signature must fail.
        sector[12] ^= 1;
        assert_eq!(nxbd::verify(&sector, &pk), Err(FmtError::Signature));
        sector[12] ^= 1;
        // Signature tamper.
        sector[500] ^= 1;
        assert_eq!(nxbd::verify(&sector, &pk), Err(FmtError::Signature));
        sector[500] ^= 1;
        // Wrong key.
        let (other_pk, _) = nxbd::pubkey_id_for_seed(&[12u8; 32]);
        assert_eq!(nxbd::verify(&sector, &other_pk), Err(FmtError::Signature));
        // Reserved bytes must be zero (fail closed).
        let mut dirty = sector;
        dirty[200] = 1;
        assert_eq!(nxbd::verify(&dirty, &pk), Err(FmtError::Reserved));
    }

    #[test]
    fn nxbd_zeroed_sector_is_invalid() {
        // Factory-empty slot: never bootable.
        assert_eq!(nxbd::decode(&[0u8; 512]).map(|_| ()), Err(FmtError::Malformed));
    }

    #[test]
    fn bsb_roundtrip_and_factory_golden() {
        let f = Bsb::factory();
        let block = bsb::encode(&f);
        assert_eq!(bsb::decode(&block).expect("decode"), f);
        // Golden anchor bytes (layout freeze): magic + seq=1 + ver + a/none.
        assert_eq!(&block[0..8], bsb::MAGIC);
        assert_eq!(block[8], 1);
        assert_eq!(block[18], 0);
        assert_eq!(block[19], 0xFF);
        assert_eq!(block[21], 1);
    }

    #[test]
    fn bsb_pick_rule_and_torn_block() {
        let mut a = Bsb::factory();
        let mut b = Bsb::factory();
        a.seq = 4;
        b.seq = 5;
        b.next_slot = Some(Slot::B);
        b.tries_left = 2;
        let (picked, idx) = bsb::pick(&bsb::encode(&a), &bsb::encode(&b)).expect("pick");
        assert_eq!((picked, idx), (b, 1));
        // Torn write on the newer block: the OLDER one stays authoritative.
        let mut torn = bsb::encode(&b);
        torn[30] ^= 0xFF;
        let (fallback, idx) = bsb::pick(&bsb::encode(&a), &torn).expect("pick torn");
        assert_eq!((fallback, idx), (a, 0));
        // Both torn: no selection (loader panics loudly, never guesses).
        let mut torn0 = bsb::encode(&a);
        torn0[0] = 0;
        assert!(bsb::pick(&torn0, &torn).is_none());
    }

    #[test]
    fn bsb_rejects_bad_fields() {
        let mut block = bsb::encode(&Bsb::factory());
        block[18] = 7; // invalid slot — recompute CRC so the field check fires
        let crc = {
            // reuse the module's polynomial via encode of a scratch? simplest:
            // flip via decode expectation — recompute manually
            let mut crc: u32 = !0;
            for &byte in &block[..508] {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg();
                    crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                }
            }
            !crc
        };
        block[508..512].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(bsb::decode(&block), Err(FmtError::Malformed));
    }
}
