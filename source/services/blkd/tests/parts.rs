// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which partitions the block owner serves (`blkd::parts`, TASK-0260 P1): on the
//! layout every stage builds, every selector finds its volume, and none of them reaches the
//! board's boot-ROM head; a partition under another type than the layout's, or a head
//! partition in place of a volume, is never served.
//! OWNERS: @runtime

use blkd::parts::{windows, Window};
use storage::blockproto::{part_layout_name, PART_COUNT, PART_DATA};
use storage::gpt::{Partition, GUID_NEXUS_FW};
use storage::layout::plan;

const HEAD: [&str; 4] = ["fsbl", "env", "opensbi", "uboot"];

#[test]
fn every_selector_finds_its_volume_on_the_layout() {
    let table = plan().expect("layout");
    let served = windows(&table);
    for sel in 0..PART_COUNT {
        let name = part_layout_name(sel).expect("a name");
        let p = table.iter().find(|p| p.name == name).expect("in the layout");
        let want = Window { first_lba: p.first_lba, sectors: p.last_lba - p.first_lba + 1 };
        assert_eq!(served[sel as usize], Some(want), "{name}");
    }
}

#[test]
fn test_reject_the_boot_rom_head_as_a_served_partition() {
    let table = plan().expect("layout");
    let head: Vec<&Partition> = table.iter().filter(|p| HEAD.contains(&p.name.as_str())).collect();
    assert_eq!(head.len(), 4);
    // No selector names a head partition …
    for sel in 0..=u8::MAX {
        assert!(part_layout_name(sel).is_none_or(|name| !HEAD.contains(&name)), "{sel}");
    }
    // … and no served window reaches one.
    for window in windows(&table).iter().flatten() {
        let last = window.first_lba + window.sectors - 1;
        for p in &head {
            assert!(last < p.first_lba || window.first_lba > p.last_lba, "{} overlapped", p.name);
        }
    }
    // A disk that has only the head serves nothing.
    let only_head: Vec<Partition> = head.into_iter().cloned().collect();
    assert!(windows(&only_head).iter().all(Option::is_none));
}

#[test]
fn test_reject_a_volume_under_another_type() {
    let mut table = plan().expect("layout");
    let data = table.iter_mut().find(|p| p.name == "data").expect("data");
    data.type_guid = GUID_NEXUS_FW;
    let served = windows(&table);
    assert_eq!(served[PART_DATA as usize], None, "the name alone is not enough");
    assert_eq!(served.iter().flatten().count(), usize::from(PART_COUNT) - 1);
}
