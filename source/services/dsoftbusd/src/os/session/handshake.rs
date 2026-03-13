//! Handshake helper material for deterministic bring-up.

/// SECURITY: bring-up test keys, NOT production custody.
pub(crate) fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[0] = tag;
    seed[1] = (port >> 8) as u8;
    seed[2] = (port & 0xff) as u8;
    for i in 3..32 {
        seed[i] = ((tag as u16).wrapping_mul(port).wrapping_add(i as u16) & 0xff) as u8;
    }
    seed
}
