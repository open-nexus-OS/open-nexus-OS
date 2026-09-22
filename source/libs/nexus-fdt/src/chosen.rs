// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ONE write path into a tree — `/chosen/nexus,*`, set in place by
//! nxboot (RFC-0098 C2). It works inside headroom the FIT build reserved (`dtc -p`)
//! and never relocates a block: the strings block must be last (it is, on every
//! producer we know); otherwise the edit is refused with `Error::Layout`. A
//! property that already exists with the same length is overwritten in place;
//! otherwise the old one is removed and a new one inserted right after the
//! `/chosen` node header, shifting everything behind it by the property's span.
//!
//! Every read of the tree happens through a short-lived [`Fdt`] view whose numbers
//! are copied out before the buffer is mutated — the borrow checker is the proof
//! that no stale view survives a shift.

use crate::header::{align4, Error, Fdt, HEADER_LEN};
use crate::platform::NexusKey;
use crate::{FDT_BEGIN_NODE, FDT_END_NODE, FDT_NOP, FDT_PROP};

/// In-place editor for `/chosen`.
pub struct ChosenWriter<'a> {
    buf: &'a mut [u8],
}

/// The header numbers a mutation needs, copied out of a view.
#[derive(Clone, Copy)]
struct Dims {
    totalsize: usize,
    off_struct: usize,
    size_struct: usize,
    off_strings: usize,
    size_strings: usize,
    /// End of the strings block = end of the last block. Bytes from here to
    /// `buf.len()` are headroom: `dtc -p` padding inside `totalsize`, or a
    /// buffer the boot stage made larger than the tree.
    used: usize,
}

impl<'a> ChosenWriter<'a> {
    /// Validate the tree in `buf`; `buf.len() - totalsize` is the headroom.
    pub fn new(buf: &'a mut [u8]) -> Result<Self, Error> {
        Fdt::new(buf)?;
        Ok(ChosenWriter { buf })
    }

    /// Set `/chosen/nexus,<name>` to a string (NUL-terminated in the tree).
    pub fn set_nexus_str(&mut self, name: &str, value: &str) -> Result<(), Error> {
        let mut key = NexusKey::new(name).ok_or(Error::NoHeadroom)?;
        let mut tmp = [0u8; 128];
        if value.len() + 1 > tmp.len() {
            return Err(Error::NoHeadroom);
        }
        tmp[..value.len()].copy_from_slice(value.as_bytes());
        self.set(key.as_str(), &tmp[..value.len() + 1])
    }

    /// Set `/chosen/nexus,<name>` to a u64 (two big-endian cells).
    pub fn set_nexus_u64(&mut self, name: &str, value: u64) -> Result<(), Error> {
        let mut key = NexusKey::new(name).ok_or(Error::NoHeadroom)?;
        self.set(key.as_str(), &value.to_be_bytes())
    }

    /// Set `/chosen/nexus,<name>` to raw bytes (bounded only by the headroom).
    pub fn set_nexus_bytes(&mut self, name: &str, value: &[u8]) -> Result<(), Error> {
        let mut key = NexusKey::new(name).ok_or(Error::NoHeadroom)?;
        self.set(key.as_str(), value)
    }

    /// The tree as it stands now (headroom excluded).
    pub fn as_fdt(&self) -> Result<Fdt<'_>, Error> {
        Fdt::new(self.buf)
    }

    fn dims(&self) -> Result<Dims, Error> {
        let fdt = Fdt::new(self.buf)?;
        let used = fdt.off_strings + fdt.size_strings;
        // The strings block must be the last block: the structure block and the
        // reservation block both end before it, or an edit would overwrite them.
        if fdt.off_struct + fdt.size_struct > used || fdt.off_rsvmap > used {
            return Err(Error::Layout);
        }
        Ok(Dims {
            totalsize: fdt.totalsize,
            off_struct: fdt.off_struct,
            size_struct: fdt.size_struct,
            off_strings: fdt.off_strings,
            size_strings: fdt.size_strings,
            used,
        })
    }

    /// Bytes an edit may still grow into.
    pub fn headroom(&self) -> usize {
        self.dims().map(|d| self.buf.len() - d.used).unwrap_or(0)
    }

    fn set(&mut self, name: &str, value: &[u8]) -> Result<(), Error> {
        // Locate /chosen and any existing property — numbers only leave this block.
        let (chosen_off, existing) = {
            let fdt = Fdt::new(self.buf)?;
            let chosen = fdt.node_at_path("/chosen").ok_or(Error::NotFound)?;
            (chosen.offset, find_prop(&fdt, chosen.offset, name)?)
        };

        if let Some((prop_off, len)) = existing {
            if len == value.len() {
                self.buf[prop_off + 12..prop_off + 12 + len].copy_from_slice(value);
                return Ok(());
            }
            self.remove_span(prop_off, 12 + align4(len))?;
        }

        let nameoff = self.ensure_string(name)?;
        let insert_at = {
            let fdt = Fdt::new(self.buf)?;
            fdt.node_header(chosen_off)?.1
        };
        let d = self.dims()?;
        let span = 12 + align4(value.len());
        if d.used + span > self.buf.len() {
            return Err(Error::NoHeadroom);
        }
        // Shift everything from the insertion point to the end of the last block.
        self.buf.copy_within(insert_at..d.used, insert_at + span);
        let b = &mut self.buf[insert_at..insert_at + span];
        b[..4].copy_from_slice(&FDT_PROP.to_be_bytes());
        b[4..8].copy_from_slice(&(value.len() as u32).to_be_bytes());
        b[8..12].copy_from_slice(&(nameoff as u32).to_be_bytes());
        b[12..12 + value.len()].copy_from_slice(value);
        for pad in &mut b[12 + value.len()..] {
            *pad = 0;
        }
        self.set_word(9, (d.size_struct + span) as u32);
        self.set_word(3, (d.off_strings + span) as u32);
        self.set_word(1, d.totalsize.max(d.used + span) as u32);
        Ok(())
    }

    /// Delete `span` bytes at `off` inside the structure block.
    fn remove_span(&mut self, off: usize, span: usize) -> Result<(), Error> {
        let d = self.dims()?;
        if off < d.off_struct || off + span > d.off_struct + d.size_struct {
            return Err(Error::Structure);
        }
        self.buf.copy_within(off + span..d.used, off);
        // The freed bytes become headroom; totalsize keeps the padding it had.
        for b in &mut self.buf[d.used - span..d.used] {
            *b = 0;
        }
        self.set_word(9, (d.size_struct - span) as u32);
        self.set_word(3, (d.off_strings - span) as u32);
        Ok(())
    }

    /// Offset of `name` in the strings block, appending it when absent (strings
    /// block must be last).
    fn ensure_string(&mut self, name: &str) -> Result<usize, Error> {
        let d = self.dims()?;
        let found = {
            let block = &self.buf[d.off_strings..d.off_strings + d.size_strings];
            let mut off = 0;
            let mut hit = None;
            while off < block.len() {
                let end = block[off..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| off + p)
                    .unwrap_or(block.len());
                if &block[off..end] == name.as_bytes() {
                    hit = Some(off);
                    break;
                }
                off = end + 1;
            }
            hit
        };
        if let Some(off) = found {
            return Ok(off);
        }
        let add = name.len() + 1;
        if d.used + add > self.buf.len() {
            return Err(Error::NoHeadroom);
        }
        let at = d.used;
        self.buf[at..at + name.len()].copy_from_slice(name.as_bytes());
        self.buf[at + name.len()] = 0;
        self.set_word(8, (d.size_strings + add) as u32);
        self.set_word(1, d.totalsize.max(d.used + add) as u32);
        Ok(d.size_strings)
    }

    fn set_word(&mut self, i: usize, v: u32) {
        debug_assert!(i * 4 + 4 <= HEADER_LEN);
        self.buf[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
}

/// `(offset, value_len)` of the property `name` directly under the node at
/// `node_off`, walking its own properties only.
fn find_prop(fdt: &Fdt<'_>, node_off: usize, name: &str) -> Result<Option<(usize, usize)>, Error> {
    let (_, mut off) = fdt.node_header(node_off)?;
    let end = fdt.off_struct + fdt.size_struct;
    while off + 4 <= end {
        match fdt.u32_at(off)? {
            FDT_PROP => {
                let len = fdt.u32_at(off + 4)? as usize;
                let nameoff = fdt.u32_at(off + 8)? as usize;
                if fdt.string(nameoff)? == name {
                    return Ok(Some((off, len)));
                }
                off = align4(off + 12 + len);
            }
            FDT_NOP => off += 4,
            FDT_BEGIN_NODE | FDT_END_NODE => return Ok(None),
            _ => return Err(Error::Structure),
        }
    }
    Ok(None)
}
