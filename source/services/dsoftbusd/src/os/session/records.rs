//! Fixed-size encrypted record contract for cross-VM gateway.

pub(crate) const TAGLEN: usize = 16;
pub(crate) const MAX_REQ: usize = 256;
pub(crate) const MAX_RSP: usize = 512;
pub(crate) const REQ_PLAIN: usize = 1 + 2 + MAX_REQ;
pub(crate) const RSP_PLAIN: usize = 1 + 2 + MAX_RSP;
pub(crate) const REQ_CIPH: usize = REQ_PLAIN + TAGLEN;
pub(crate) const RSP_CIPH: usize = RSP_PLAIN + TAGLEN;
