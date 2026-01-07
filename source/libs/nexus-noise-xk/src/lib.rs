// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal no_std Noise XK handshake + transport keys (X25519 + ChaChaPoly + BLAKE2s)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (Phase 1; stable framing in Phase 2)
//! TEST_COVERAGE: 3 host tests (happy path, key mismatch, bad lengths)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! RFC: docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
//!
//! Notes:
//! - This module is intentionally **small** and tailored to the OS/QEMU bring-up needs.
//! - It is used by `dsoftbusd` and `selftest-client` for deterministic handshake proof.
//! - It does not implement the full Noise framework — only the **XK** pattern and a tiny transport API.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use blake2::digest::Digest;
use blake2::Blake2s256;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Tag};
use x25519_dalek::{PublicKey, StaticSecret};

pub const PROTOCOL_NAME: &str = "Noise_XK_25519_ChaChaPoly_BLAKE2s";

pub const DHLEN: usize = 32;
pub const HASHLEN: usize = 32;
pub const TAGLEN: usize = 16;

pub const MSG1_LEN: usize = 32;
pub const MSG2_LEN: usize = 32 + (32 + TAGLEN) + TAGLEN; // e_r || enc(s_r) || enc(payload="")
pub const MSG3_LEN: usize = (32 + TAGLEN) + TAGLEN; // enc(s_i) || enc(payload="")

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseError {
    BadLength,
    Crypto,
    StaticKeyMismatch,
}

#[derive(Clone, Copy)]
pub struct StaticKeypair {
    pub secret: [u8; 32],
    pub public: [u8; 32],
}

impl StaticKeypair {
    pub fn from_secret(secret: [u8; 32]) -> Self {
        let sec = StaticSecret::from(secret);
        let pubk = PublicKey::from(&sec);
        Self { secret: sec.to_bytes(), public: pubk.to_bytes() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportKeys {
    pub send: [u8; 32],
    pub recv: [u8; 32],
}

#[derive(Clone, Copy)]
struct CipherState {
    key: Option<[u8; 32]>,
    nonce: u64,
}

impl CipherState {
    fn new() -> Self {
        Self { key: None, nonce: 0 }
    }

    fn initialize_key(&mut self, key: [u8; 32]) {
        self.key = Some(key);
        self.nonce = 0;
    }

    fn encrypt_with_ad(
        &mut self,
        ad: &[u8],
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, NoiseError> {
        if plaintext.len() + TAGLEN > out.len() {
            return Err(NoiseError::BadLength);
        }
        let Some(key_bytes) = self.key else {
            out[..plaintext.len()].copy_from_slice(plaintext);
            return Ok(plaintext.len());
        };

        out[..plaintext.len()].copy_from_slice(plaintext);
        let key = Key::from_slice(&key_bytes);
        let aead = ChaCha20Poly1305::new(key);
        let nonce = make_nonce(self.nonce);
        let tag = aead
            .encrypt_in_place_detached((&nonce).into(), ad, &mut out[..plaintext.len()])
            .map_err(|_| NoiseError::Crypto)?;
        out[plaintext.len()..plaintext.len() + TAGLEN].copy_from_slice(tag.as_slice());
        self.nonce = self.nonce.wrapping_add(1);
        Ok(plaintext.len() + TAGLEN)
    }

    fn decrypt_with_ad(
        &mut self,
        ad: &[u8],
        ciphertext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, NoiseError> {
        let Some(key_bytes) = self.key else {
            if ciphertext.len() > out.len() {
                return Err(NoiseError::BadLength);
            }
            out[..ciphertext.len()].copy_from_slice(ciphertext);
            return Ok(ciphertext.len());
        };

        if ciphertext.len() < TAGLEN {
            return Err(NoiseError::BadLength);
        }
        let pt_len = ciphertext.len() - TAGLEN;
        if pt_len > out.len() {
            return Err(NoiseError::BadLength);
        }
        out[..pt_len].copy_from_slice(&ciphertext[..pt_len]);
        let key = Key::from_slice(&key_bytes);
        let aead = ChaCha20Poly1305::new(key);
        let nonce = make_nonce(self.nonce);
        let mut tag_bytes = [0u8; TAGLEN];
        tag_bytes.copy_from_slice(&ciphertext[pt_len..]);
        let tag = Tag::from_slice(&tag_bytes);
        aead.decrypt_in_place_detached((&nonce).into(), ad, &mut out[..pt_len], tag)
            .map_err(|_| NoiseError::Crypto)?;
        self.nonce = self.nonce.wrapping_add(1);
        Ok(pt_len)
    }
}

#[derive(Clone, Copy)]
struct SymmetricState {
    ck: [u8; HASHLEN],
    h: [u8; HASHLEN],
    cipher: CipherState,
}

impl SymmetricState {
    fn initialize(protocol_name: &[u8]) -> Self {
        let mut h = [0u8; HASHLEN];
        if protocol_name.len() <= HASHLEN {
            h[..protocol_name.len()].copy_from_slice(protocol_name);
        } else {
            h.copy_from_slice(&hash(protocol_name));
        }
        Self { ck: h, h, cipher: CipherState::new() }
    }

    fn mix_hash(&mut self, data: &[u8]) {
        let mut hasher = Blake2s256::new();
        hasher.update(self.h);
        hasher.update(data);
        self.h.copy_from_slice(&hasher.finalize());
    }

    fn mix_key(&mut self, ikm: &[u8]) {
        let (ck, temp_k) = hkdf2(&self.ck, ikm);
        self.ck = ck;
        self.cipher.initialize_key(temp_k);
    }

    fn encrypt_and_hash(&mut self, plaintext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        let n = self.cipher.encrypt_with_ad(&self.h, plaintext, out)?;
        self.mix_hash(&out[..n]);
        Ok(n)
    }

    fn decrypt_and_hash(&mut self, ciphertext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        let n = self.cipher.decrypt_with_ad(&self.h, ciphertext, out)?;
        self.mix_hash(ciphertext);
        Ok(n)
    }

    fn split(&self) -> ([u8; 32], [u8; 32]) {
        // Noise Split: HKDF(ck, zero) -> k1, k2
        let (k1, k2) = hkdf2(&self.ck, &[]);
        (k1, k2)
    }
}

pub struct XkInitiator {
    local_static: StaticKeypair,
    remote_static_pub: [u8; 32],
    e: StaticSecret,
    e_pub: [u8; 32],
    symmetric: SymmetricState,
    re_pub: Option<[u8; 32]>,
}

impl XkInitiator {
    /// `eph_seed` is a deterministic bring-up seed for the ephemeral key (NOT secure; Phase 2 replaces this).
    pub fn new(
        local_static: StaticKeypair,
        remote_static_pub: [u8; 32],
        eph_seed: [u8; 32],
    ) -> Self {
        let mut symmetric = SymmetricState::initialize(PROTOCOL_NAME.as_bytes());
        // XK has responder static as a pre-message.
        symmetric.mix_hash(&remote_static_pub);
        let e = StaticSecret::from(eph_seed);
        let e_pub = PublicKey::from(&e).to_bytes();
        Self { local_static, remote_static_pub, e, e_pub, symmetric, re_pub: None }
    }

    pub fn write_msg1(&mut self, out: &mut [u8; MSG1_LEN]) {
        out.copy_from_slice(&self.e_pub);
        self.symmetric.mix_hash(out);
    }

    pub fn read_msg2_write_msg3(
        &mut self,
        msg2: &[u8],
        out_msg3: &mut [u8; MSG3_LEN],
    ) -> Result<TransportKeys, NoiseError> {
        if msg2.len() != MSG2_LEN {
            return Err(NoiseError::BadLength);
        }
        let mut re = [0u8; 32];
        re.copy_from_slice(&msg2[..32]);
        self.re_pub = Some(re);
        self.symmetric.mix_hash(&re);

        // ee
        let dh_ee = dh(&self.e, &re);
        self.symmetric.mix_key(&dh_ee);

        // decrypt responder static (must match pinned key)
        let mut rs_plain = [0u8; 32];
        let n = self.symmetric.decrypt_and_hash(&msg2[32..80], &mut rs_plain)?;
        if n != 32 || rs_plain != self.remote_static_pub {
            return Err(NoiseError::StaticKeyMismatch);
        }

        // es = DH(e, rs)
        let dh_es = dh(&self.e, &self.remote_static_pub);
        self.symmetric.mix_key(&dh_es);

        // decrypt payload (empty)
        let mut scratch = [0u8; 1];
        let n = self.symmetric.decrypt_and_hash(&msg2[80..96], &mut scratch)?;
        if n != 0 {
            return Err(NoiseError::BadLength);
        }

        // msg3: enc(s_i)
        let mut off = 0;
        let n = self
            .symmetric
            .encrypt_and_hash(&self.local_static.public, &mut out_msg3[off..off + 48])?;
        if n != 48 {
            return Err(NoiseError::BadLength);
        }
        off += n;

        // se = DH(s_i, re)
        let dh_se = dh_static(&self.local_static.secret, &re);
        self.symmetric.mix_key(&dh_se);

        // enc(payload="") -> tag only
        let n = self.symmetric.encrypt_and_hash(&[], &mut out_msg3[off..off + 16])?;
        if n != 16 || off + n != MSG3_LEN {
            return Err(NoiseError::BadLength);
        }

        let (k1, k2) = self.symmetric.split();
        Ok(TransportKeys { send: k1, recv: k2 })
    }
}

pub struct XkResponder {
    local_static: StaticKeypair,
    expected_remote_static_pub: [u8; 32],
    e: StaticSecret,
    e_pub: [u8; 32],
    symmetric: SymmetricState,
    ie_pub: Option<[u8; 32]>,
}

impl XkResponder {
    /// `eph_seed` is a deterministic bring-up seed for the ephemeral key (NOT secure; Phase 2 replaces this).
    pub fn new(
        local_static: StaticKeypair,
        expected_remote_static_pub: [u8; 32],
        eph_seed: [u8; 32],
    ) -> Self {
        let mut symmetric = SymmetricState::initialize(PROTOCOL_NAME.as_bytes());
        // XK has responder static as a pre-message (the responder knows its own static).
        symmetric.mix_hash(&local_static.public);
        let e = StaticSecret::from(eph_seed);
        let e_pub = PublicKey::from(&e).to_bytes();
        Self { local_static, expected_remote_static_pub, e, e_pub, symmetric, ie_pub: None }
    }

    pub fn read_msg1_write_msg2(
        &mut self,
        msg1: &[u8],
        out_msg2: &mut [u8; MSG2_LEN],
    ) -> Result<(), NoiseError> {
        if msg1.len() != MSG1_LEN {
            return Err(NoiseError::BadLength);
        }
        let mut ie = [0u8; 32];
        ie.copy_from_slice(msg1);
        self.ie_pub = Some(ie);
        self.symmetric.mix_hash(&ie);

        // e_r
        out_msg2[..32].copy_from_slice(&self.e_pub);
        self.symmetric.mix_hash(&out_msg2[..32]);

        // ee = DH(e_r, e_i)
        let dh_ee = dh(&self.e, &ie);
        self.symmetric.mix_key(&dh_ee);

        // enc(s_r)
        let n =
            self.symmetric.encrypt_and_hash(&self.local_static.public, &mut out_msg2[32..80])?;
        if n != 48 {
            return Err(NoiseError::BadLength);
        }

        // es = DH(s_r, e_i)
        let dh_es = dh_static(&self.local_static.secret, &ie);
        self.symmetric.mix_key(&dh_es);

        // enc(payload="") -> tag only
        let n = self.symmetric.encrypt_and_hash(&[], &mut out_msg2[80..96])?;
        if n != 16 {
            return Err(NoiseError::BadLength);
        }

        Ok(())
    }

    pub fn read_msg3_finish(&mut self, msg3: &[u8]) -> Result<TransportKeys, NoiseError> {
        if msg3.len() != MSG3_LEN {
            return Err(NoiseError::BadLength);
        }
        if self.ie_pub.is_none() {
            return Err(NoiseError::BadLength);
        }

        // decrypt initiator static
        let mut is_plain = [0u8; 32];
        let n = self.symmetric.decrypt_and_hash(&msg3[..48], &mut is_plain)?;
        if n != 32 || is_plain != self.expected_remote_static_pub {
            return Err(NoiseError::StaticKeyMismatch);
        }

        // se = DH(e_r, s_i)
        let dh_se = dh(&self.e, &is_plain);
        self.symmetric.mix_key(&dh_se);

        // decrypt payload (empty)
        let mut scratch = [0u8; 1];
        let n = self.symmetric.decrypt_and_hash(&msg3[48..64], &mut scratch)?;
        if n != 0 {
            return Err(NoiseError::BadLength);
        }

        let (k1, k2) = self.symmetric.split();
        // Responder direction is reversed.
        Ok(TransportKeys { send: k2, recv: k1 })
    }
}

#[derive(Clone, Copy)]
pub struct Transport {
    send: CipherState,
    recv: CipherState,
}

impl Transport {
    pub fn new(keys: TransportKeys) -> Self {
        let mut send = CipherState::new();
        send.initialize_key(keys.send);
        let mut recv = CipherState::new();
        recv.initialize_key(keys.recv);
        Self { send, recv }
    }

    pub fn encrypt(&mut self, plaintext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        self.send.encrypt_with_ad(&[], plaintext, out)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        self.recv.decrypt_with_ad(&[], ciphertext, out)
    }
}

fn make_nonce(n: u64) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[4..12].copy_from_slice(&n.to_le_bytes());
    out
}

fn dh(secret: &StaticSecret, public_bytes: &[u8; 32]) -> [u8; 32] {
    let pk = PublicKey::from(*public_bytes);
    secret.diffie_hellman(&pk).to_bytes()
}

fn dh_static(secret_bytes: &[u8; 32], public_bytes: &[u8; 32]) -> [u8; 32] {
    let sec = StaticSecret::from(*secret_bytes);
    dh(&sec, public_bytes)
}

fn hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2s256::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&out);
    buf
}

fn hmac_blake2s(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k0 = [0u8; BLOCK];
    if key.len() > BLOCK {
        k0[..32].copy_from_slice(&hash(key));
    } else {
        k0[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; BLOCK];
    let mut opad = [0u8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] = k0[i] ^ 0x36;
        opad[i] = k0[i] ^ 0x5c;
    }

    let mut inner = Blake2s256::new();
    inner.update(ipad);
    inner.update(data);
    let inner = inner.finalize();

    let mut outer = Blake2s256::new();
    outer.update(opad);
    outer.update(inner);
    let out = outer.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&out);
    buf
}

fn hkdf2(chaining_key: &[u8; 32], ikm: &[u8]) -> ([u8; 32], [u8; 32]) {
    let prk = hmac_blake2s(chaining_key, ikm);
    let t1 = hmac_blake2s(&prk, &[0x01]);
    let mut t1_2 = [0u8; 33];
    t1_2[..32].copy_from_slice(&t1);
    t1_2[32] = 0x02;
    let t2 = hmac_blake2s(&prk, &t1_2);
    (t1, t2)
}
