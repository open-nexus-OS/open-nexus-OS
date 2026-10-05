// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sectors through ADMA2 and PIO, under the cache protocol.
//! OWNERS: @runtime @drivers

use storage_sdhci::Ceiling;

use crate::model::card::pattern;
use crate::model::mem::{CacheOp, ModelCache, ModelMem};
use crate::model::{self, commands};

fn sectors(lba: u32, n: usize) -> Vec<u8> {
    (0..n).flat_map(|s| (0..512).map(move |i| pattern(lba + s as u32, i))).collect()
}

#[test]
fn unwritten_sectors_read_back_as_the_cards_own_data() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    let mut buf = vec![0u8; 8 * 512];
    disk.read(4096, &mut buf).expect("read");
    assert_eq!(buf, sectors(4096, 8));
}

#[test]
fn adma_writes_and_reads_round_trip_through_scattered_buffers_and_a_non_coherent_cache() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    // 200 sectors: a full 128-sector chunk and a 72-sector one, through four bounce runs.
    let data: Vec<u8> = (0..200 * 512).map(|i| (i * 13 + i / 512) as u8).collect();
    disk.write(1000, &data).expect("write");
    for (i, sector) in data.chunks_exact(512).enumerate() {
        assert_eq!(
            &m.borrow().card.written[&(1000 + i as u32)][..],
            sector,
            "sector {i} on the card"
        );
    }
    let mut back = vec![0u8; data.len()];
    disk.read(1000, &mut back).expect("read");
    assert_eq!(back, data);
    let issued = commands(&m);
    let tail: Vec<u8> = issued[issued.len() - 4..].iter().map(|c| c.0).collect();
    assert_eq!(tail, [23, 18, 23, 18], "each chunk is CMD23 then CMD18");
}

#[test]
fn every_transfer_takes_both_buffers_through_the_cache_protocol() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    let (table, bounce) = (0, 1);
    m.borrow_mut().mem.ops.clear();
    disk.read(0, &mut [0u8; 1024]).expect("read");
    assert_eq!(
        m.borrow().mem.ops,
        [
            CacheOp::Clean(table, 0, 4096),
            CacheOp::Flush(bounce, 0, 65536),
            CacheOp::Flush(bounce, 0, 65536)
        ],
        "descriptors cleaned; the bounce flushed before the device writes and again after"
    );
    m.borrow_mut().mem.ops.clear();
    disk.write(0, &[7u8; 1024]).expect("write");
    assert_eq!(
        m.borrow().mem.ops,
        [CacheOp::Clean(table, 0, 4096), CacheOp::Clean(bounce, 0, 65536)],
        "descriptors and data cleaned; nothing to do after the device read"
    );
}

#[test]
fn pio_reads_need_no_dma_memory_and_no_interrupt() {
    let m = model::machine(model::qemu());
    let host = model::host(&m, false);
    let mut card =
        storage_sdhci::Card::init(host, Ceiling::Hs52).map_err(|f| f.error).expect("init");
    let mut buf = vec![0u8; 3 * 512];
    card.read_pio(77, &mut buf).expect("pio");
    assert_eq!(buf, sectors(77, 3));
    assert!(m.borrow().mem.ops.is_empty());
}

#[test]
fn the_cache_model_catches_a_missing_clean_and_a_missing_flush() {
    use nexus_driverkit::{CacheOps, DmaMemory};
    let m = model::machine(model::k1());
    let mut mem = ModelMem::new(&m, 4096, 0x4000_0000, 1, 0);
    let cache = ModelCache(m.clone());
    let mut seen = [0u8; 4];
    // The CPU wrote; without a clean the device reads what DRAM held.
    mem.bytes_mut()[..4].copy_from_slice(&[1, 2, 3, 4]);
    m.borrow().mem.read(0x4000_0000, &mut seen).unwrap();
    assert_eq!(seen, [0; 4]);
    cache.clean(mem.bytes(), 64);
    m.borrow().mem.read(0x4000_0000, &mut seen).unwrap();
    assert_eq!(seen, [1, 2, 3, 4]);
    // The device wrote; without a flush the CPU reads its stale view.
    m.borrow_mut().mem.write(0x4000_0000, &[9, 9, 9, 9]).unwrap();
    assert_eq!(&mem.bytes()[..4], &[1, 2, 3, 4]);
    cache.flush(mem.bytes_mut(), 64);
    assert_eq!(&mem.bytes()[..4], &[9, 9, 9, 9]);
    // A dirty line flushed after the device wrote overwrites the device's data: the
    // reason a buffer is flushed BEFORE the device writes it.
    mem.bytes_mut()[0] = 5;
    m.borrow_mut().mem.write(0x4000_0000, &[8, 8, 8, 8]).unwrap();
    cache.flush(mem.bytes_mut(), 64);
    assert_eq!(&mem.bytes()[..4], &[5, 8, 8, 8]);
}
