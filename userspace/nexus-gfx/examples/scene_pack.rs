// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Packs a capture directory into a scene bundle (TASK-0169B P1, RFC-0101 §8c).
//!
//! The directory is what the shim's hook wrote (`docs/board/measurements/2026-10-13-gpu-first-
//! quad/capture/pvr_dump_hook.h`): `jobN.stream`, `jobN.txt` (`type= context= flags= …`) and
//! `jobN-bo-<va>.bin` for every buffer bound at that submit. A buffer bound at several submits
//! must only have GAINED bytes between them (the driver appends programs; it never rewrites
//! what an earlier job read), so the last version serves every job; the packer refuses a
//! capture where that does not hold. The buffer whose bytes are the checker texture becomes
//! the `Texture` (generated on the device); the one of a plane's size becomes the `Target`
//! (bound to the ring, never carried); the rest are `Data` as sparse runs.
//!
//! ```text
//! cargo run -p nexus-gfx --example scene_pack -- <capture-dir> <out.scene>
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use nexus_gfx::backend::gpu::scene::{
    checker_v1, fnv1a64, runs_of, Builder, JobKind, Kind, Scene, Texture, CHECKER_V1_FNV1A64,
};
use nexus_gfx::backend::gpu::CoreId;

/// A plane of the ring: 1080 rows of 7680 bytes (`nexus_display_proto::layout`).
const PLANE_BYTES: usize = 1080 * 7680;
const TEXTURE: (u32, u32, u32) = (256, 256, 1024);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, out) = match args.as_slice() {
        [_, dir, out] => (Path::new(dir), Path::new(out)),
        _ => {
            eprintln!("usage: scene_pack <capture-dir> <out.scene>");
            std::process::exit(2);
        }
    };
    let mut versions: BTreeMap<u64, Vec<(u32, Vec<u8>)>> = BTreeMap::new();
    let mut jobs: BTreeMap<u32, (u32, u32, Vec<u8>)> = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("capture directory") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let Some(rest) = name.strip_prefix("job") else { continue };
        if let Some((n, va)) = rest.strip_suffix(".bin").and_then(|r| r.split_once("-bo-")) {
            let n: u32 = n.parse().expect("job number");
            let va = u64::from_str_radix(va, 16).expect("buffer address");
            versions.entry(va).or_default().push((n, std::fs::read(&path).expect("buffer")));
        } else if let Some(n) = rest.strip_suffix(".stream") {
            let n: u32 = n.parse().expect("job number");
            let stream = std::fs::read(&path).expect("stream");
            let meta =
                std::fs::read_to_string(dir.join(format!("job{n}.txt"))).expect("job header");
            let field = |key: &str| -> u32 {
                let v = meta
                    .split_whitespace()
                    .find_map(|kv| kv.strip_prefix(key))
                    .unwrap_or_else(|| panic!("job{n}.txt lacks {key}"));
                match v.strip_prefix("0x") {
                    Some(h) => u32::from_str_radix(h, 16).expect("hex"),
                    None => v.parse().expect("number"),
                }
            };
            jobs.insert(n, (field("type="), field("flags="), stream));
        }
    }
    let mut checker = vec![0u8; (TEXTURE.1 * TEXTURE.2) as usize];
    checker_v1(TEXTURE.0, TEXTURE.1, TEXTURE.2, &mut checker);
    assert_eq!(fnv1a64(&checker), CHECKER_V1_FNV1A64, "the formula drifted from its golden");

    let mut b = Builder::new(CoreId::BOARD, Texture::None);
    let mut texture = false;
    let mut rows = Vec::new();
    for (va, mut vs) in versions {
        vs.sort_by_key(|(n, _)| *n);
        let last = &vs[vs.len() - 1].1;
        for (n, earlier) in &vs[..vs.len() - 1] {
            assert_eq!(earlier.len(), last.len(), "buffer {va:#x} changed size after job {n}");
            for (i, (&e, &l)) in earlier.iter().zip(last).enumerate() {
                assert!(
                    e == 0 || e == l,
                    "buffer {va:#x} byte {i:#x} rewritten after job {n} ({e:#x} -> {l:#x}): not append-only"
                );
            }
        }
        let len = last.len() as u64;
        let (kind, runs) = if *last == checker {
            texture = true;
            (Kind::Texture, Vec::new())
        } else if last.len() == PLANE_BYTES {
            (Kind::Target, Vec::new())
        } else {
            (Kind::Data, runs_of(last, 32))
        };
        let carried: usize = runs.iter().map(|(_, r)| r.len()).sum();
        rows.push(format!("| `{va:#014x}` | {len:>9} | {kind:?} | {} | {carried} |", runs.len()));
        b.buffer(va, len, kind, &runs);
    }
    let mut built = b;
    if texture {
        // Rebuild with the texture formula named: the builder fixes it in the header.
        let bytes = built.finish();
        let mut again = Builder::new(CoreId::BOARD, Texture::CheckerV1);
        let s = Scene::parse(&bytes, CoreId::BOARD).expect("intermediate bundle");
        for buf in s.buffers() {
            let runs: Vec<(u32, Vec<u8>)> = buf.runs().map(|r| (r.off, r.bytes.to_vec())).collect();
            again.buffer(buf.va, buf.len, buf.kind, &runs);
        }
        built = again;
    }
    for (n, (ty, flags, stream)) in &jobs {
        assert_eq!(*ty, 3, "job {n}: only transfer jobs (type 3) are packed in this layer");
        built.job(JobKind::Transfer, *flags, stream);
    }
    let bytes = built.finish();
    let scene =
        Scene::parse(&bytes, CoreId::BOARD).expect("the bundle parses for the board's core");
    std::fs::write(out, &bytes).expect("write bundle");
    println!("| GPU address | Bytes | Kind | Runs | Carried |");
    println!("|---|---|---|---|---|");
    for r in rows {
        println!("{r}");
    }
    println!(
        "\n{} buffers, {} jobs, {} data bytes carried, bundle {} bytes -> {}",
        scene.buffer_count,
        scene.job_count,
        scene.data_bytes(),
        bytes.len(),
        out.display()
    );
}
