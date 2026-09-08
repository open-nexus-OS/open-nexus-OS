// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Accept-side CIDR allow-list (RFC-0092 §3): a peer is admitted
//! only when it lies in one declared `cidr_allow` entry; an empty list
//! admits nobody (fail closed — the grammar never produces one).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: unit tests below, tests/ingress_host/ (`test_reject_cidr`)

use crate::table::Cidr;

/// `ip ∈ cidr` (a `len > 32` entry matches nothing).
pub fn contains(cidr: &Cidr, ip: [u8; 4]) -> bool {
    if cidr.len > 32 {
        return false;
    }
    let mask = if cidr.len == 0 { 0 } else { u32::MAX << (32 - u32::from(cidr.len)) };
    (u32::from_be_bytes(ip) & mask) == (u32::from_be_bytes(cidr.addr) & mask)
}

/// `true` when some entry admits `ip`.
pub fn allowed(list: &[Cidr], ip: [u8; 4]) -> bool {
    list.iter().any(|c| contains(c, ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_match_is_exact_on_the_mask() {
        let net = Cidr { addr: [10, 0, 2, 0], len: 24 };
        assert!(contains(&net, [10, 0, 2, 2]));
        assert!(contains(&net, [10, 0, 2, 255]));
        assert!(!contains(&net, [10, 0, 3, 1]));
        let host = Cidr { addr: [10, 0, 2, 2], len: 32 };
        assert!(contains(&host, [10, 0, 2, 2]));
        assert!(!contains(&host, [10, 0, 2, 3]));
        let all = Cidr { addr: [0, 0, 0, 0], len: 0 };
        assert!(contains(&all, [203, 0, 113, 9]));
        assert!(!contains(&Cidr { addr: [0, 0, 0, 0], len: 33 }, [1, 2, 3, 4]));
    }

    #[test]
    fn empty_list_admits_nobody() {
        assert!(!allowed(&[], [10, 0, 2, 2]));
        assert!(allowed(&[Cidr { addr: [10, 0, 2, 0], len: 24 }], [10, 0, 2, 2]));
    }
}
