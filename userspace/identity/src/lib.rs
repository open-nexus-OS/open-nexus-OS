//! CONTEXT: Device identity and signing support for Open Nexus OS userland services
//! INTENT: Generate stable device identifiers, sign/verify messages, persist keys
//! IDL (target): generate(), fromJson(json), toJson(), sign(message), verify(message,sig)
//! DEPS: ed25519-dalek (crypto), serde (serialization), rand-core (entropy)
//! READINESS: Host backend ready; OS backend needs persistent storage
//! TESTS: Identity generation, JSON roundtrip, signature verification
//!
//! The identity module provides helpers for generating a stable device
//! identifier derived from the long-term signing key. Keys can be persisted by
//! serialising the identity to JSON. The same APIs work for the host tooling and
//! the operating system userland where persistent storage will be added later.

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

use core::fmt;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use rand_core::OsRng;
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Errors produced by identity operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// The stored representation could not be parsed.
    Deserialize(String),
    /// Serialising the identity failed.
    Serialize(String),
    /// A signing or verification primitive failed.
    Crypto(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityError::Deserialize(msg)
            | IdentityError::Serialize(msg)
            | IdentityError::Crypto(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Stable textual identifier derived from the public key hash.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    /// Creates a device id from a verifying key.
    pub fn from_verifying_key(key: &VerifyingKey) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(key.to_bytes());
        let digest = hasher.finalize();
        DeviceId(hex::encode(digest))
    }

    /// Returns the underlying identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    signing_key: String,
}

/// Device identity holding the signing keypair and cached identifier.
#[derive(Clone)]
pub struct Identity {
    device_id: DeviceId,
    signing_key: SigningKey,
}

impl Identity {
    /// Generates a new identity using the provided random number generator.
    pub fn generate_with<R>(rng: &mut R) -> Result<Self, IdentityError>
    where
        R: RngCore + CryptoRng,
    {
        let signing_key = SigningKey::generate(rng);
        Ok(Self::from_signing_key(signing_key))
    }

    /// Generates a new identity using the operating system random source.
    pub fn generate() -> Result<Self, IdentityError> {
        let mut rng = OsRng;
        Self::generate_with(&mut rng)
    }

    /// Reconstructs an identity from raw signing key bytes.
    pub fn from_secret_key_bytes(bytes: &[u8; 32]) -> Result<Self, IdentityError> {
        let signing_key = SigningKey::from_bytes(bytes);
        Ok(Self::from_signing_key(signing_key))
    }

    /// Restores an identity from its JSON representation.
    pub fn from_json(json: &str) -> Result<Self, IdentityError> {
        let stored: StoredIdentity = serde_json::from_str(json)
            .map_err(|err| IdentityError::Deserialize(err.to_string()))?;
        let bytes = hex::decode(stored.signing_key)
            .map_err(|err| IdentityError::Deserialize(err.to_string()))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IdentityError::Deserialize("invalid signing key length".into()))?;
        Self::from_secret_key_bytes(&bytes)
    }

    /// Serialises the identity as JSON for storage.
    pub fn to_json(&self) -> Result<String, IdentityError> {
        let stored = StoredIdentity { signing_key: hex::encode(self.signing_key.to_bytes()) };
        serde_json::to_string(&stored).map_err(|err| IdentityError::Serialize(err.to_string()))
    }

    /// Returns the stable device identifier.
    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    /// Returns the verifying key associated with this identity.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Returns the raw signing key bytes. Useful for persistence layers.
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Signs the provided message and returns the detached signature.
    pub fn sign(&self, message: &[u8]) -> Signature {
        use ed25519_dalek::Signer;
        self.signing_key.sign(message)
    }

    /// Verifies a signature against this identity's verifying key.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        Self::verify_with_key(&self.verifying_key(), message, signature)
    }

    /// Verifies a signature using the provided verifying key.
    pub fn verify_with_key(key: &VerifyingKey, message: &[u8], signature: &Signature) -> bool {
        use ed25519_dalek::Verifier;
        key.verify(message, signature).is_ok()
    }

    fn from_signing_key(signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        let device_id = DeviceId::from_verifying_key(&verifying_key);
        Self { device_id, signing_key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_device_id_and_signs() {
        let identity = Identity::generate().expect("identity generation");
        assert!(!identity.device_id().as_str().is_empty());

        let message = b"hello nexus";
        let signature = identity.sign(message);
        assert!(identity.verify(message, &signature));
    }

    #[test]
    fn round_trips_json() {
        let identity = Identity::generate().expect("identity generation");
        let json = identity.to_json().expect("serialize");
        let restored = Identity::from_json(&json).expect("deserialize");
        assert_eq!(identity.device_id(), restored.device_id());
        assert_eq!(identity.secret_key_bytes(), restored.secret_key_bytes());

        let message = b"round trip";
        let signature = identity.sign(message);
        assert!(Identity::verify_with_key(&restored.verifying_key(), message, &signature));
    }
}
