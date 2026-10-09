// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pulled-write contract of the nxfs engine (TASK-0068):
//! `Nxfs::write_from` takes its content from a source the engine asks piece
//! by piece — how vfsd writes a client's VMO without holding its bytes —
//! and must be indistinguishable from `write` over a slice, roll back
//! completely when the source fails, and bound every write before the
//! source is asked for a byte.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, public API only)

use nxfs::{MkfsOptions, Nxfs, NxfsError, LOGICAL_BLOCK_SIZE, MAX_FILE_BYTES};
use storage::{BlockDevice, MemBlockDevice};

fn content(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// One write sequence — an unaligned rewrite crossing an existing file's
/// head and tail, then a small interior one — through either surface.
fn run(pulled: bool) -> MemBlockDevice {
    let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 4096);
    let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
    fs.create("/f").expect("create");
    fs.write("/f", 0, &[7u8; 10_000]).expect("seed");
    let data = content(300_001);
    for (offset, bytes) in [(5_000u64, &data[..]), (123, &data[..77])] {
        if pulled {
            // Every byte asked for exactly once, in order.
            let mut next = 0u64;
            fs.write_from("/f", offset, bytes.len() as u64, &mut |at, out| {
                assert_eq!(at, next, "pieces arrive in order, without gaps or repeats");
                next += out.len() as u64;
                out.copy_from_slice(&bytes[at as usize..next as usize]);
                Ok(())
            })
            .expect("pulled write");
            assert_eq!(next, bytes.len() as u64, "every byte was asked for");
        } else {
            fs.write("/f", offset, bytes).expect("slice write");
        }
    }
    fs.into_device()
}

/// `write` IS `write_from` over a slice: the same sequence through either
/// surface leaves byte-identical devices — data, journal and superblocks.
#[test]
fn pulled_write_equals_slice_write() {
    let (sliced, pulled) = (run(false), run(true));
    let mut a = vec![0u8; LOGICAL_BLOCK_SIZE];
    let mut b = vec![0u8; LOGICAL_BLOCK_SIZE];
    for lb in 0..sliced.block_count() {
        sliced.read_block(lb, &mut a).expect("read sliced");
        pulled.read_block(lb, &mut b).expect("read pulled");
        assert_eq!(a, b, "block {lb}");
    }
}

/// A source that fails mid-write aborts it before anything commits: its
/// error comes back as it was, the file reads exactly as before, and no
/// fresh block leaks — the volume is sized so ONE leaked 60-block rewrite
/// would make the next one `NoSpace`.
#[test]
fn test_reject_failing_source_rolls_back() {
    // 256 blocks leave 158 data blocks: the file plus one CoW rewrite fit.
    let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 256);
    let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
    let size = 60 * LOGICAL_BLOCK_SIZE;
    fs.create("/f").expect("create");
    fs.write("/f", 0, &vec![1u8; size]).expect("seed");

    for err in [NxfsError::Io, NxfsError::Invalid] {
        let mut asked = 0;
        let result = fs.write_from("/f", 0, size as u64, &mut |_, out| {
            asked += 1;
            if asked == 3 {
                return Err(err); // mid-write: two chunks are already on disk
            }
            out.fill(2);
            Ok(())
        });
        assert_eq!(result, Err(err), "the source's error comes back unchanged");
        assert_eq!(fs.stat("/f").expect("stat").1, size as u64);
        let bytes = fs.read("/f", 0, size).expect("read");
        assert!(bytes.iter().all(|b| *b == 1), "{err:?}: the file is unchanged");
    }
    fs.write_from("/f", 0, size as u64, &mut |_, out| {
        out.fill(3);
        Ok(())
    })
    .expect("no fresh block leaked by the failed writes");
    assert!(fs.read("/f", 0, size).expect("read").iter().all(|b| *b == 3));
}

/// Every bound is checked before the source is asked for a byte; a
/// zero-length write is a no-op that asks for nothing.
#[test]
fn test_reject_write_from_out_of_bounds() {
    let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 4096);
    let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
    fs.create("/f").expect("create");
    fs.mkdir("/d").expect("mkdir");
    let mut asked = 0u32;
    let mut source = |_: u64, _: &mut [u8]| -> nxfs::Result<()> {
        asked += 1;
        Ok(())
    };
    let cases: [(&str, u64, u64, NxfsError); 6] = [
        ("/f", 0, MAX_FILE_BYTES + 1, NxfsError::TooBig),
        ("/f", MAX_FILE_BYTES, 1, NxfsError::TooBig),
        ("/f", u64::MAX, 1, NxfsError::TooBig),
        ("/d", 0, 1, NxfsError::IsDir),
        ("/nope", 0, 1, NxfsError::NotFound),
        // Ending exactly AT the cap passes the size bound; on this 16 MiB
        // volume the write then fails for space — allocation comes before
        // the first pull.
        ("/f", MAX_FILE_BYTES - 1, 1, NxfsError::NoSpace),
    ];
    for (path, offset, len, err) in cases {
        assert_eq!(
            fs.write_from(path, offset, len, &mut source),
            Err(err),
            "{path}@{offset}+{len}"
        );
    }
    assert_eq!(fs.write_from("/f", 0, 0, &mut source), Ok(()));
    assert_eq!(asked, 0, "the source was never asked");
    assert_eq!(fs.stat("/f").expect("stat").1, 0);
}
