// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which partitions the owner serves (ADR-0044, RFC-0089 §2): the protocol's
//! selectors, each found in the parsed GPT by its layout name AND the type the layout gives
//! that name — the loader's rule (`find_partition_named`) — so a partition of the right name
//! and another type is not served. The board's boot-ROM head (`fsbl`, `env`, `opensbi`,
//! `uboot`, TASK-0260 P1) has no selector: no request can name it. Pure, so the host proves
//! it (`tests/parts.rs`).
//! OWNERS: @runtime

use storage::blockproto::{self, PART_COUNT};
use storage::gpt::{find_partition_named, Partition};
use storage::layout;

/// A served partition's window on the disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub first_lba: u64,
    pub sectors: u64,
}

/// The windows of the protocol's selectors in `table`, indexed by selector; a selector whose
/// name is missing, or present under another type, has none.
pub fn windows(table: &[Partition]) -> [Option<Window>; PART_COUNT as usize] {
    let mut out = [None; PART_COUNT as usize];
    for (sel, slot) in (0..PART_COUNT).zip(out.iter_mut()) {
        let Some(name) = blockproto::part_layout_name(sel) else { continue };
        let Some(type_guid) = layout::type_of(name) else { continue };
        if let Some(p) = find_partition_named(table, &type_guid, name) {
            *slot = Some(Window { first_lba: p.first_lba, sectors: p.last_lba - p.first_lba + 1 });
        }
    }
    out
}
