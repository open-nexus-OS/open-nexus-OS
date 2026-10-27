// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Policyd app-grants frames (RFC-0111 §6, ADR-0084): the bundle manager PUTs one installed
//! app's manifest grants to the one permission authority, the launchers GET them. v2 nonce
//! frames on the policyd wire; every item bounded (`MAX_APP_ID_BYTES`, `MAX_GRANT_BYTES`,
//! `MAX_APP_GRANTS`) and the list checked strictly (`GrantList::is_well_formed`) before a
//! decoder hands it out. Re-exported from [`crate::policyd`] — split out for the structure
//! ratchet, same protocol.

use crate::policyd::{
    Nonce, MAGIC0, MAGIC1, MAX_APP_GRANTS, MAX_APP_ID_BYTES, MAX_GRANT_BYTES, OP_APP_GRANTS_GET,
    OP_APP_GRANTS_PUT, STATUS_ALLOW, VERSION_V2,
};

/// The grants carried by an app-grants frame: `n:u8, (len:u8, name...)*n`, read lazily and
/// bounds-checked per item (an over-long or truncated item ends the iteration with `None`
/// — callers that need the strict form check [`GrantList::is_well_formed`]).
#[derive(Clone, Copy, Debug)]
pub struct GrantList<'a> {
    left: u8,
    rest: &'a [u8],
}

impl<'a> GrantList<'a> {
    /// Parses the list tail; `None` when the count exceeds [`MAX_APP_GRANTS`].
    fn parse(r: &mut crate::codec::Reader<'a>) -> Option<Self> {
        let left = r.take_u8()?;
        if left as usize > MAX_APP_GRANTS {
            return None;
        }
        let rest = r.take_bytes(r.remaining())?;
        let list = Self { left, rest };
        if !list.is_well_formed() {
            return None;
        }
        Some(list)
    }

    /// The declared item count.
    pub fn len(&self) -> usize {
        self.left as usize
    }

    /// `true` when the list declares no grants.
    pub fn is_empty(&self) -> bool {
        self.left == 0
    }

    /// `true` iff exactly `len` bounded items fill the tail with no bytes left over.
    pub fn is_well_formed(&self) -> bool {
        let mut it = *self;
        let n = it.by_ref().count();
        n == self.len() && it.rest.is_empty()
    }
}

impl<'a> Iterator for GrantList<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.left == 0 {
            return None;
        }
        let mut r = crate::codec::Reader::new(self.rest);
        let item = r.take_len8_bytes(1, MAX_GRANT_BYTES)?;
        self.rest = &self.rest[r.pos()..];
        self.left -= 1;
        Some(item)
    }
}

fn put_grants(w: &mut crate::codec::Writer<'_>, grants: &[&[u8]]) -> Option<()> {
    if grants.len() > MAX_APP_GRANTS {
        return None;
    }
    w.put_u8(grants.len() as u8)?;
    for g in grants {
        w.put_len8_bytes(g, 1, MAX_GRANT_BYTES)?;
    }
    Some(())
}

/// Encodes an app-grants PUT request (v2):
/// `[P,O,2,OP_APP_GRANTS_PUT, nonce:u32le, app_len:u8, app..., n:u8, (len:u8, name...)*n]`.
pub fn encode_app_grants_put(
    nonce: Nonce,
    app: &[u8],
    grants: &[&[u8]],
    out: &mut [u8],
) -> Option<usize> {
    let mut w = crate::codec::Writer::new(out);
    crate::codec::put_hdr(&mut w, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_PUT)?;
    w.put_u32le(nonce)?;
    w.put_len8_bytes(app, 1, MAX_APP_ID_BYTES)?;
    put_grants(&mut w, grants)?;
    Some(w.pos())
}

/// Decodes an app-grants PUT request → `(nonce, app, grants)`.
pub fn decode_app_grants_put(frame: &[u8]) -> Option<(Nonce, &[u8], GrantList<'_>)> {
    let mut r = crate::codec::Reader::new(frame);
    crate::codec::check_hdr(&mut r, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_PUT)?;
    let nonce = r.take_u32le()?;
    let app = r.take_len8_bytes(1, MAX_APP_ID_BYTES)?;
    let grants = GrantList::parse(&mut r)?;
    Some((nonce, app, grants))
}

/// Encodes an app-grants GET request (v2):
/// `[P,O,2,OP_APP_GRANTS_GET, nonce:u32le, app_len:u8, app...]`.
pub fn encode_app_grants_get(nonce: Nonce, app: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut w = crate::codec::Writer::new(out);
    crate::codec::put_hdr(&mut w, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_GET)?;
    w.put_u32le(nonce)?;
    w.put_len8_bytes(app, 1, MAX_APP_ID_BYTES)?;
    Some(w.pos())
}

/// Decodes an app-grants GET request → `(nonce, app)`.
pub fn decode_app_grants_get(frame: &[u8]) -> Option<(Nonce, &[u8])> {
    let mut r = crate::codec::Reader::new(frame);
    crate::codec::check_hdr(&mut r, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_GET)?;
    let nonce = r.take_u32le()?;
    let app = r.take_len8_bytes(1, MAX_APP_ID_BYTES)?;
    r.finish_exact()?;
    Some((nonce, app))
}

/// Encodes the GET answer:
/// `[P,O,2,OP_APP_GRANTS_GET|0x80, nonce:u32le, status:u8, n:u8, (len:u8, name...)*n]`
/// — `n` is 0 on every status but `STATUS_ALLOW`.
pub fn encode_app_grants_rsp(
    nonce: Nonce,
    status: u8,
    grants: &[&[u8]],
    out: &mut [u8],
) -> Option<usize> {
    let mut w = crate::codec::Writer::new(out);
    crate::codec::put_hdr(&mut w, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_GET | 0x80)?;
    w.put_u32le(nonce)?;
    w.put_u8(status)?;
    put_grants(&mut w, if status == STATUS_ALLOW { grants } else { &[] })?;
    Some(w.pos())
}

/// Decodes the GET answer → `(nonce, status, grants)`.
pub fn decode_app_grants_rsp(frame: &[u8]) -> Option<(Nonce, u8, GrantList<'_>)> {
    let mut r = crate::codec::Reader::new(frame);
    crate::codec::check_hdr(&mut r, MAGIC0, MAGIC1, VERSION_V2, OP_APP_GRANTS_GET | 0x80)?;
    let nonce = r.take_u32le()?;
    let status = r.take_u8()?;
    let grants = GrantList::parse(&mut r)?;
    Some((nonce, status, grants))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policyd::STATUS_DENY;

    #[test]
    fn app_grants_put_get_roundtrip() {
        let grants: [&[u8]; 2] = [b"nexus.permission.WINDOW", b"nexus.permission.BACKGROUND"];
        let mut buf = [0u8; 256];
        let n = encode_app_grants_put(7, b"notes", &grants, &mut buf).unwrap();
        let (nonce, app, list) = decode_app_grants_put(&buf[..n]).unwrap();
        assert_eq!((nonce, app, list.len()), (7, &b"notes"[..], 2));
        assert_eq!(first_four(list), [grants[0], grants[1], &[], &[]]);
        // Zero grants is a valid, well-formed list.
        let n = encode_app_grants_put(8, b"plain", &[], &mut buf).unwrap();
        let (_, _, list) = decode_app_grants_put(&buf[..n]).unwrap();
        assert!(list.is_empty() && list.is_well_formed());
        // The GET request and its answer.
        let n = encode_app_grants_get(9, b"notes", &mut buf).unwrap();
        assert_eq!(decode_app_grants_get(&buf[..n]).unwrap(), (9, &b"notes"[..]));
        let n = encode_app_grants_rsp(9, STATUS_ALLOW, &grants, &mut buf).unwrap();
        let (nonce, status, list) = decode_app_grants_rsp(&buf[..n]).unwrap();
        assert_eq!((nonce, status, list.len()), (9, STATUS_ALLOW, 2));
        // A refusal carries no grants, whatever the encoder was handed.
        let n = encode_app_grants_rsp(9, STATUS_DENY, &grants, &mut buf).unwrap();
        let (_, status, list) = decode_app_grants_rsp(&buf[..n]).unwrap();
        assert_eq!((status, list.len()), (STATUS_DENY, 0));
    }

    /// The first four items of a list, empty slices past its end (alloc-free collect).
    fn first_four(mut list: GrantList<'_>) -> [&[u8]; 4] {
        let mut out: [&[u8]; 4] = [&[]; 4];
        for slot in out.iter_mut() {
            match list.next() {
                Some(g) => *slot = g,
                None => break,
            }
        }
        out
    }

    #[test]
    fn app_grants_reject_bounds_and_truncation() {
        let mut buf = [0u8; 1024];
        // Too many grants, an over-long name, an empty name, an empty app id: refused.
        let many: [&[u8]; MAX_APP_GRANTS + 1] = [b"nexus.permission.WINDOW"; MAX_APP_GRANTS + 1];
        assert!(encode_app_grants_put(1, b"a", &many, &mut buf).is_none());
        let long = [b'x'; MAX_GRANT_BYTES + 1];
        assert!(encode_app_grants_put(1, b"a", &[&long], &mut buf).is_none());
        assert!(encode_app_grants_put(1, b"a", &[b""], &mut buf).is_none());
        assert!(encode_app_grants_put(1, b"", &[], &mut buf).is_none());
        // A hand-built list whose count overstates its items is not well formed.
        let n = encode_app_grants_put(1, b"a", &[b"nexus.permission.WINDOW"], &mut buf).unwrap();
        let mut forged = [0u8; 64];
        forged[..n].copy_from_slice(&buf[..n]);
        forged[4 + 4 + 1 + 1] = 2; // n: 1 → 2 with one item present
        assert!(decode_app_grants_put(&forged[..n]).is_none());
        // Truncation / mutation matrix on the full frames.
        let grants: [&[u8]; 1] = [b"nexus.permission.WINDOW"];
        let n = encode_app_grants_put(3, b"notes", &grants, &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_app_grants_put(f).is_some()
        });
        let n = encode_app_grants_rsp(3, STATUS_ALLOW, &grants, &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_app_grants_rsp(f).is_some()
        });
    }
}
