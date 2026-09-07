// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §5 learn record — the ONE line format policyd emits
//! (logd scope `policyd.learn`) and `nx policy learn-gen` parses:
//! `abi.learn epoch=<u32> subject=<sid hex16> class=<statefs|net.bind|net.connect>
//!  arg=<prefix|port/loopback|port/any|a.b.c.d:port> would=<deny|limit>`.
//! `core`-only on purpose: the host `policy` crate uses it as a module and
//! policyd's OS-lite build includes this file by path (`#[path]`), so the
//! writer and the reader can never disagree. Also home of
//! `service_id_from_name` (FNV-1a 64), the subject-name → id map every
//! policy tool shares.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; policyd `abi_eval` tests;
//!   tools/nx/tests/policy_cli.rs
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

/// Longest emitted record (RFC-0091 §5).
pub const MAX_LEARN_RECORD_BYTES: usize = 160;
/// Longest statefs prefix a record carries (mirrors `MAX_PATH_PREFIX_BYTES`).
pub const MAX_LEARN_PREFIX_BYTES: usize = 64;

const RECORD_TAG: &str = "abi.learn";

/// Governed class of a learn record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LearnClass {
    /// `statefs.put`.
    Statefs,
    /// `net.bind` / listen / udp-bind.
    NetBind,
    /// `net.connect`.
    NetConnect,
}

impl LearnClass {
    /// Record token.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Statefs => "statefs",
            Self::NetBind => "net.bind",
            Self::NetConnect => "net.connect",
        }
    }
}

/// Bind address class token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LearnAddr {
    /// Loopback-only bind.
    Loopback,
    /// Any-interface bind (needs `--allow-any` at generation).
    Any,
}

/// The observed argument, class-shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LearnArg<'a> {
    /// Directory prefix of the denied path (`…/`), ≤ 64 bytes, printable ASCII.
    Statefs(&'a str),
    /// Denied bind port + address class.
    NetBind(u16, LearnAddr),
    /// Denied connect destination.
    NetConnect([u8; 4], u16),
}

/// Why the evaluation would refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Would {
    /// No accepting allow rule (or a deny rule won).
    Deny,
    /// An allow matched but a `limits` ceiling was exceeded.
    Limit,
}

/// One learn record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LearnRecord<'a> {
    /// Profile epoch the observation was made under.
    pub epoch: u32,
    /// Kernel-attributed subject id.
    pub subject: u64,
    /// Observed argument (its variant is the class).
    pub arg: LearnArg<'a>,
    /// Refusal kind.
    pub would: Would,
}

/// FNV-1a 64 over a service name — the subject id every policy table uses.
pub const fn service_id_from_name(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < name.len() {
        h ^= name[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// The learn prefix of a canonical statefs path: everything up to and
/// including the last `/` that still fits `MAX_LEARN_PREFIX_BYTES`.
/// `None` when the path is not absolute, not printable ASCII (a record is
/// one space-separated line) or has no directory part inside the bound.
pub fn statefs_learn_prefix(path: &[u8]) -> Option<&str> {
    if path.first() != Some(&b'/') || !path.iter().all(|b| (0x21..=0x7e).contains(b)) {
        return None;
    }
    let window = &path[..path.len().min(MAX_LEARN_PREFIX_BYTES)];
    let cut = window.iter().rposition(|b| *b == b'/')? + 1;
    core::str::from_utf8(&path[..cut]).ok()
}

struct Writer<'a> {
    out: &'a mut [u8],
    len: usize,
    overflow: bool,
}

impl Writer<'_> {
    fn push(&mut self, bytes: &[u8]) {
        if self.len + bytes.len() > self.out.len() {
            self.overflow = true;
            return;
        }
        self.out[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
    fn dec(&mut self, mut v: u64) {
        let mut d = [0u8; 20];
        let mut n = 0;
        loop {
            d[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.push(&d[n..n + 1]);
        }
    }
    fn hex16(&mut self, v: u64) {
        const H: &[u8; 16] = b"0123456789abcdef";
        let mut b = [0u8; 16];
        for (i, slot) in b.iter_mut().enumerate() {
            *slot = H[((v >> (60 - 4 * i)) & 0xf) as usize];
        }
        self.push(&b);
    }
}

impl LearnRecord<'_> {
    /// Record class.
    pub const fn class(&self) -> LearnClass {
        match self.arg {
            LearnArg::Statefs(_) => LearnClass::Statefs,
            LearnArg::NetBind(..) => LearnClass::NetBind,
            LearnArg::NetConnect(..) => LearnClass::NetConnect,
        }
    }

    /// Writes the one-line record (no newline); `None` when it would not
    /// fit `out` or exceed `MAX_LEARN_RECORD_BYTES`.
    pub fn write(&self, out: &mut [u8]) -> Option<usize> {
        let mut w = Writer { out, len: 0, overflow: false };
        w.push(RECORD_TAG.as_bytes());
        w.push(b" epoch=");
        w.dec(self.epoch as u64);
        w.push(b" subject=");
        w.hex16(self.subject);
        w.push(b" class=");
        w.push(self.class().token().as_bytes());
        w.push(b" arg=");
        match self.arg {
            LearnArg::Statefs(prefix) => {
                if prefix.is_empty()
                    || prefix.len() > MAX_LEARN_PREFIX_BYTES
                    || !prefix.bytes().all(|b| (0x21..=0x7e).contains(&b))
                {
                    return None;
                }
                w.push(prefix.as_bytes());
            }
            LearnArg::NetBind(port, addr) => {
                w.dec(port as u64);
                w.push(match addr {
                    LearnAddr::Loopback => b"/loopback",
                    LearnAddr::Any => b"/any",
                });
            }
            LearnArg::NetConnect(a, port) => {
                for (i, o) in a.iter().enumerate() {
                    if i > 0 {
                        w.push(b".");
                    }
                    w.dec(*o as u64);
                }
                w.push(b":");
                w.dec(port as u64);
            }
        }
        w.push(b" would=");
        w.push(match self.would {
            Would::Deny => b"deny",
            Would::Limit => b"limit",
        });
        if w.overflow || w.len > MAX_LEARN_RECORD_BYTES {
            return None;
        }
        Some(w.len)
    }
}

fn parse_u16(s: &str) -> Option<u16> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: u32 = 0;
    for b in s.bytes() {
        v = v * 10 + (b - b'0') as u32;
    }
    u16::try_from(v).ok()
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: u64 = 0;
    for b in s.bytes() {
        v = v * 10 + (b - b'0') as u64;
    }
    u32::try_from(v).ok()
}

fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u64;
    }
    Some(v)
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut n = 0;
    for part in s.split('.') {
        if n == 4 || part.is_empty() || part.len() > 3 {
            return None;
        }
        out[n] = u8::try_from(parse_u16(part)?).ok()?;
        n += 1;
    }
    (n == 4).then_some(out)
}

/// Parses one record line (surrounding whitespace ignored). Anything that
/// is not exactly the format above yields `None` — a generator never
/// guesses.
pub fn parse_learn_record(line: &str) -> Option<LearnRecord<'_>> {
    let line = line.trim();
    let mut parts = line.split(' ').filter(|p| !p.is_empty());
    if parts.next()? != RECORD_TAG {
        return None;
    }
    let epoch = parse_u32(parts.next()?.strip_prefix("epoch=")?)?;
    let subject = parse_hex16(parts.next()?.strip_prefix("subject=")?)?;
    let class = parts.next()?.strip_prefix("class=")?;
    let arg = parts.next()?.strip_prefix("arg=")?;
    let would = match parts.next()?.strip_prefix("would=")? {
        "deny" => Would::Deny,
        "limit" => Would::Limit,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let arg = match class {
        "statefs" => {
            if !arg.starts_with('/')
                || !arg.ends_with('/')
                || arg.len() > MAX_LEARN_PREFIX_BYTES
                || !arg.bytes().all(|b| (0x21..=0x7e).contains(&b))
            {
                return None;
            }
            LearnArg::Statefs(arg)
        }
        "net.bind" => {
            let (port, addr) = arg.split_once('/')?;
            let addr = match addr {
                "loopback" => LearnAddr::Loopback,
                "any" => LearnAddr::Any,
                _ => return None,
            };
            LearnArg::NetBind(parse_u16(port)?, addr)
        }
        "net.connect" => {
            let (ip, port) = arg.rsplit_once(':')?;
            LearnArg::NetConnect(parse_ipv4(ip)?, parse_u16(port)?)
        }
        _ => return None,
    };
    Some(LearnRecord { epoch, subject, arg, would })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(r: LearnRecord<'_>) -> String {
        let mut buf = [0u8; MAX_LEARN_RECORD_BYTES];
        let n = r.write(&mut buf).unwrap();
        let line = core::str::from_utf8(&buf[..n]).unwrap().to_string();
        assert_eq!(parse_learn_record(&line).unwrap(), r);
        line
    }

    #[test]
    fn record_format_is_the_contract() {
        let r = LearnRecord {
            epoch: 3,
            subject: 0x0102_0304_0506_0708,
            arg: LearnArg::Statefs("/state/app/x/"),
            would: Would::Deny,
        };
        assert_eq!(
            roundtrip(r),
            "abi.learn epoch=3 subject=0102030405060708 class=statefs arg=/state/app/x/ would=deny"
        );
        let r = LearnRecord {
            epoch: 0,
            subject: u64::MAX,
            arg: LearnArg::NetBind(8080, LearnAddr::Any),
            would: Would::Deny,
        };
        assert_eq!(
            roundtrip(r),
            "abi.learn epoch=0 subject=ffffffffffffffff class=net.bind arg=8080/any would=deny"
        );
        let r = LearnRecord {
            epoch: 7,
            subject: 1,
            arg: LearnArg::NetConnect([10, 0, 2, 2], 443),
            would: Would::Limit,
        };
        assert_eq!(
            roundtrip(r),
            "abi.learn epoch=7 subject=0000000000000001 class=net.connect arg=10.0.2.2:443 would=limit"
        );
    }

    #[test]
    fn parser_rejects_anything_else() {
        for bad in [
            "",
            "abi.learn",
            "abi.learn epoch=1 subject=0000000000000001 class=statefs arg=/x/ would=deny extra",
            "abi.learn epoch=1 subject=0000000000000001 class=regex arg=/x/ would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=statefs arg=x/ would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=statefs arg=/x would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=net.bind arg=70000/any would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=net.bind arg=80/wan would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=net.connect arg=10.0.2:80 would=deny",
            "abi.learn epoch=1 subject=00000000000000zz class=statefs arg=/x/ would=deny",
            "abi.learn epoch=1 subject=0000000000000001 class=statefs arg=/x/ would=maybe",
            "audit v1 op=check decision=deny subject=0x1",
        ] {
            assert!(parse_learn_record(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn prefix_derivation_is_bounded_and_literal() {
        assert_eq!(statefs_learn_prefix(b"/state/app/x/token"), Some("/state/app/x/"));
        assert_eq!(statefs_learn_prefix(b"/token"), Some("/"));
        assert_eq!(statefs_learn_prefix(b"state/app"), None);
        assert_eq!(statefs_learn_prefix(b"/state/app x/t"), None);
        let mut long = b"/state/".to_vec();
        long.extend(core::iter::repeat(b'a').take(100));
        long.extend_from_slice(b"/t");
        assert_eq!(statefs_learn_prefix(&long), Some("/state/"));
    }

    #[test]
    fn writer_never_overflows() {
        let r = LearnRecord {
            epoch: 1,
            subject: 1,
            arg: LearnArg::Statefs("/state/app/x/"),
            would: Would::Deny,
        };
        assert!(r.write(&mut [0u8; 16]).is_none());
        let too_long = "/".repeat(MAX_LEARN_PREFIX_BYTES + 1);
        let r = LearnRecord { arg: LearnArg::Statefs(&too_long), ..r };
        assert!(r.write(&mut [0u8; 256]).is_none());
    }

    #[test]
    fn service_id_matches_the_table_hash() {
        // The FNV-1a value policyd's table was built with for this name.
        assert_eq!(service_id_from_name(b""), 0xcbf29ce484222325);
        assert_ne!(service_id_from_name(b"selftest-client"), service_id_from_name(b"policyd"));
    }
}
