//! Pure entry helpers extracted for host-testable logic checks.

pub(crate) const QEMU_USERNET_FALLBACK_IP: [u8; 4] = [10, 0, 2, 15];
pub(crate) const OS2VM_NODE_A_IP: [u8; 4] = [10, 42, 0, 10];
pub(crate) const OS2VM_NODE_B_IP: [u8; 4] = [10, 42, 0, 11];

#[inline]
pub(crate) fn is_cross_vm_ip(local_ip: [u8; 4]) -> bool {
    local_ip[0] == 10 && local_ip[1] == 42 && local_ip[2] == 0
}

#[inline]
pub(crate) fn next_nonce(n: &mut u64) -> u64 {
    let out = *n;
    *n = n.wrapping_add(1);
    out
}

pub(crate) fn rebuild_peer_ips(
    peers: &nexus_peer_lru::PeerLru,
    ips: &mut alloc::vec::Vec<(alloc::string::String, [u8; 4])>,
) {
    // Keep only entries that exist in the LRU and preserve LRU order deterministically.
    let mut out: alloc::vec::Vec<(alloc::string::String, [u8; 4])> = alloc::vec::Vec::new();
    for p in peers.peers() {
        if let Some((_id, ip)) = ips.iter().find(|(id, _)| id == &p.device_id) {
            out.push((p.device_id.clone(), *ip));
        }
    }
    *ips = out;
}

pub(crate) fn set_peer_ip(
    ips: &mut alloc::vec::Vec<(alloc::string::String, [u8; 4])>,
    device_id: &str,
    ip: [u8; 4],
) {
    if let Some((_, old)) = ips.iter_mut().find(|(id, _)| id.as_str() == device_id) {
        *old = ip;
        return;
    }
    ips.push((alloc::string::String::from(device_id), ip));
}

pub(crate) fn get_peer_ip(
    ips: &[(alloc::string::String, [u8; 4])],
    device_id: &str,
) -> Option<[u8; 4]> {
    ips.iter()
        .find(|(id, _)| id.as_str() == device_id)
        .map(|(_, ip)| *ip)
}

/// SECURITY: bring-up test keys, NOT production custody.
pub(crate) fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[0] = tag;
    seed[1] = (port >> 8) as u8;
    seed[2] = (port & 0xff) as u8;
    for (i, byte) in seed.iter_mut().enumerate().skip(3) {
        *byte = ((tag as u16).wrapping_mul(port).wrapping_add(i as u16) & 0xff) as u8;
    }
    seed
}
