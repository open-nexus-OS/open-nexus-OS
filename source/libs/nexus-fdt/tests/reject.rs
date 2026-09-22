// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the negative half of RFC-0098's parser rule — a malformed tree is a
//! typed error, never a wild read. Each test corrupts the board golden in one
//! specific way and asserts the specific refusal (or, for a corruption the header
//! cannot see, that iteration ends instead of panicking).

use nexus_fdt::{ChosenWriter, Error, Fdt};

const BOARD: &[u8] = include_bytes!("goldens/bpi-f3.dtb");

fn word(buf: &mut [u8], i: usize, v: u32) {
    buf[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
}

#[test]
fn test_reject_bad_magic() {
    let mut buf = BOARD.to_vec();
    word(&mut buf, 0, 0xdead_beef);
    assert_eq!(Fdt::new(&buf).err(), Some(Error::BadMagic));
}

#[test]
fn test_reject_truncated_buffer() {
    let total = Fdt::new(BOARD).unwrap().total_size();
    assert_eq!(Fdt::new(&BOARD[..total - 1]).err(), Some(Error::Truncated));
    assert_eq!(Fdt::new(&BOARD[..20]).err(), Some(Error::Truncated));
}

#[test]
fn test_reject_block_offset_past_totalsize() {
    let mut buf = BOARD.to_vec();
    word(&mut buf, 3, 0x7fff_fff0); // off_dt_strings
    assert_eq!(Fdt::new(&buf).err(), Some(Error::Truncated));
    let mut buf = BOARD.to_vec();
    word(&mut buf, 9, 0x7fff_fff0); // size_dt_struct
    assert_eq!(Fdt::new(&buf).err(), Some(Error::Truncated));
}

#[test]
fn test_reject_unsupported_version() {
    let mut buf = BOARD.to_vec();
    word(&mut buf, 6, 99); // last_comp_version
    assert_eq!(Fdt::new(&buf).err(), Some(Error::Version));
}

#[test]
fn test_reject_string_offset_past_strings_block() {
    // Corrupt the nameoff of the first property of the root node.
    let mut buf = BOARD.to_vec();
    Fdt::new(&buf).unwrap();
    let off_struct = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    // root: BEGIN_NODE, "" + pad → first PROP at off_struct + 8
    let prop = off_struct + 8;
    assert_eq!(u32::from_be_bytes([buf[prop], buf[prop + 1], buf[prop + 2], buf[prop + 3]]), 3);
    buf[prop + 8..prop + 12].copy_from_slice(&0x00ff_ffffu32.to_be_bytes());
    let fdt = Fdt::new(&buf).unwrap();
    // The property iterator stops at the corrupt entry instead of reading past the block.
    let root = fdt.root().unwrap();
    assert_eq!(root.props().count(), 0);
    assert!(root.prop("compatible").is_none());
}

#[test]
fn test_reject_reg_shorter_than_its_cells() {
    // A node whose `reg` has fewer bytes than #address-cells + #size-cells demand.
    let mut buf = BOARD.to_vec();
    // Find the gpu node's reg property and shrink its declared length.
    let fdt = Fdt::new(&buf).unwrap();
    let gpu = fdt.find_compatible(&["img,rgx"]).next().unwrap();
    let reg = gpu.prop("reg").unwrap();
    let reg_off = reg.as_ptr() as usize - buf.as_ptr() as usize;
    // The length word sits 8 bytes before the value.
    buf[reg_off - 8..reg_off - 4].copy_from_slice(&12u32.to_be_bytes()); // 16 → 12
    let fdt = Fdt::new(&buf).unwrap();
    let gpu = fdt.find_compatible(&["img,rgx"]).next().unwrap();
    assert_eq!(gpu.reg(0), Err(Error::ShortProp));
}

#[test]
fn test_reject_chosen_write_without_chosen_node() {
    // A tree without /chosen: nxboot must not invent one silently.
    let mut buf = BOARD.to_vec();
    buf.resize(buf.len() + 256, 0);
    // Rename the chosen node in place ("chosen" → "chosem") so lookup fails.
    let pos = buf.windows(7).position(|w| w == b"chosen\0").expect("chosen node name");
    buf[pos + 5] = b'm';
    let mut w = ChosenWriter::new(&mut buf).unwrap();
    assert_eq!(w.set_nexus_str("boot-slot", "a"), Err(Error::NotFound));
}

#[test]
fn test_reject_deeper_than_max_depth_ends_iteration() {
    // Nesting beyond MAX_DEPTH is refused by ending the walk, never by overflowing.
    let mut buf = BOARD.to_vec();
    let total = Fdt::new(&buf).unwrap().total_size();
    // Append 20 nested BEGIN_NODE tokens before FDT_END by rewriting the tail:
    // simplest deterministic corruption — replace the final END with BEGIN_NODEs.
    let off_struct = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    let size_struct = u32::from_be_bytes([buf[36], buf[37], buf[38], buf[39]]) as usize;
    let end_tok = off_struct + size_struct - 4;
    assert_eq!(
        u32::from_be_bytes([buf[end_tok], buf[end_tok + 1], buf[end_tok + 2], buf[end_tok + 3]]),
        9
    );
    let mut nested = Vec::new();
    for _ in 0..20 {
        nested.extend_from_slice(&1u32.to_be_bytes()); // BEGIN_NODE
        nested.extend_from_slice(b"x\0\0\0"); // name "x" + pad
    }
    nested.extend_from_slice(&9u32.to_be_bytes());
    buf.splice(end_tok..end_tok + 4, nested.iter().copied());
    let grown = buf.len() - total;
    // Strings block moved: patch off_dt_strings, size_dt_struct, totalsize.
    let off_strings = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]) as usize + grown;
    word(&mut buf, 3, off_strings as u32);
    word(&mut buf, 9, (size_struct + grown) as u32);
    word(&mut buf, 1, (total + grown) as u32);
    let fdt = Fdt::new(&buf).unwrap();
    let n = fdt.all_nodes().count();
    assert!(n > 0 && n < 400, "walk ended, no overflow: {n} nodes");
}

const BOARD_TREE: &[u8] = include_bytes!("goldens/bpi-f3.dtb");

/// Find the eMMC `clocks` value (`<phandle 10 phandle 13>`) in the flat tree.
fn emmc_clocks_offset(buf: &[u8]) -> (usize, u32) {
    let fdt = Fdt::new(buf).unwrap();
    let apmu = fdt.node_at_path("/soc/syscon@d4282800").unwrap().phandle().unwrap();
    let mut pat = Vec::new();
    for w in [apmu, 10, apmu, 13] {
        pat.extend_from_slice(&w.to_be_bytes());
    }
    let off = buf.windows(pat.len()).position(|w| w == pat.as_slice()).expect("clocks value");
    (off, apmu)
}

#[test]
fn test_reject_specifier_with_dangling_phandle() {
    let mut buf = BOARD_TREE.to_vec();
    let (off, _) = emmc_clocks_offset(&buf);
    buf[off..off + 4].copy_from_slice(&0xdead_beefu32.to_be_bytes());
    let fdt = Fdt::new(&buf).unwrap();
    let emmc = fdt.node_at_path("/soc/mmc@d4281000").unwrap();
    // The first entry points nowhere: the list ends there, nothing is guessed.
    assert_eq!(emmc.specifiers("clocks", "#clock-cells").count(), 0);
}

#[test]
fn test_reject_specifier_cells_beyond_the_property() {
    let mut buf = BOARD_TREE.to_vec();
    // Make the provider claim 64 cells per entry: every entry is now truncated.
    let fdt = Fdt::new(&buf).unwrap();
    let apmu = fdt.node_at_path("/soc/syscon@d4282800").unwrap();
    let cells_bytes = apmu.prop("#clock-cells").unwrap();
    let cells_off = cells_bytes.as_ptr() as usize - buf.as_ptr() as usize;
    buf[cells_off..cells_off + 4].copy_from_slice(&64u32.to_be_bytes());
    let fdt = Fdt::new(&buf).unwrap();
    let emmc = fdt.node_at_path("/soc/mmc@d4281000").unwrap();
    assert_eq!(emmc.specifiers("clocks", "#clock-cells").count(), 0);
}
