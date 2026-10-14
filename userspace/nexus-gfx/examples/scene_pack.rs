// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Packs a capture directory into a scene bundle (TASK-0169B P1 / TASK-0215 5a, RFC-0101
//! §8c/§8d).
//!
//! The directory is what the shim's hook wrote (`docs/board/measurements/2026-10-13-gpu-first-
//! quad/capture/pvr_dump_hook.h`): `jobN.stream`, `jobN.txt` (`type= context= flags= …`),
//! `jobN-bo-<va>.bin` for every buffer bound at that submit, `log.txt` with the free lists and
//! contexts, `hwrt1.args` and `context<N>.state` for a render capture. A buffer bound at several
//! submits must only have GAINED bytes between them (the driver appends programs; it never
//! rewrites what an earlier job read), so the last version serves every job; the packer refuses
//! a capture where that does not hold. The buffer whose bytes are the checker texture becomes
//! the `Texture` (generated on the device); the one of a plane's size becomes the `Target`
//! (bound to the ring, never carried); the rest are `Data` as sparse runs. A capture with
//! geometry and fragment jobs gets the render trailer: the dataset's arguments, the two free
//! lists `[local, global]` in the order the dataset names them, the render context's static
//! state.
//!
//! ```text
//! cargo run -p nexus-gfx --example scene_pack -- <capture-dir> <out.scene>
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use nexus_gfx::backend::gpu::fwif::{FreelistArgs, HwrtArgs};
use nexus_gfx::backend::gpu::scene::{
    checker_v1, fnv1a64, runs_of, Builder, JobKind, Kind, Scene, Texture, CHECKER_V1_FNV1A64,
};
use nexus_gfx::backend::gpu::CoreId;

/// A plane of the ring: 1080 rows of 7680 bytes (`nexus_display_proto::layout`).
const PLANE_BYTES: usize = 1080 * 7680;
const TEXTURE: (u32, u32, u32) = (256, 256, 1024);
/// The open driver's submit flag that makes a fragment job the partial render.
const FRAG_PARTIAL_RENDER: u32 = 1 << 6;

fn field(line: &str, key: &str) -> Option<u64> {
    let v = line.split_whitespace().find_map(|kv| kv.strip_prefix(key))?;
    match v.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => v.parse().ok(),
    }
}

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
            let head = meta.lines().next().unwrap_or("");
            let ty =
                field(head, "type=").unwrap_or_else(|| panic!("job{n}.txt lacks type=")) as u32;
            let flags =
                field(head, "flags=").unwrap_or_else(|| panic!("job{n}.txt lacks flags=")) as u32;
            jobs.insert(n, (ty, flags, stream));
        }
    }
    let mut checker = vec![0u8; (TEXTURE.1 * TEXTURE.2) as usize];
    checker_v1(TEXTURE.0, TEXTURE.1, TEXTURE.2, &mut checker);
    assert_eq!(fnv1a64(&checker), CHECKER_V1_FNV1A64, "the formula drifted from its golden");

    // The attachment is the plane-sized buffer the capture bound FIRST (the program creates it
    // before anything else); a later plane-sized zero buffer is the driver's partial-render
    // scratch and is carried as zero data.
    let log = std::fs::read_to_string(dir.join("log.txt")).unwrap_or_default();
    let mut first_plane: Option<(u64, u64)> = None; // (handle, va)
    for line in log.lines().filter(|l| l.starts_with("map ")) {
        let (Some(handle), Some(va), Some(size)) =
            (field(line, "handle="), field(line, "va="), field(line, "size="))
        else {
            continue;
        };
        if size == PLANE_BYTES as u64 && first_plane.is_none_or(|(h, _)| handle < h) {
            first_plane = Some((handle, va));
        }
    }
    let target_va = first_plane.map(|(_, va)| va);

    // The buffers: the last version of each, append-only across the submits.
    let mut buffers: Vec<(u64, u64, Kind, Vec<(u32, Vec<u8>)>)> = Vec::new();
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
        } else if last.len() == PLANE_BYTES && target_va.is_none_or(|t| t == va) {
            (Kind::Target, Vec::new())
        } else {
            (Kind::Data, runs_of(last, 32))
        };
        let carried: usize = runs.iter().map(|(_, r)| r.len()).sum();
        rows.push(format!("| `{va:#014x}` | {len:>9} | {kind:?} | {} | {carried} |", runs.len()));
        buffers.push((va, len, kind, runs));
    }

    // The render trailer, when the capture has a render pass.
    let has_render = jobs.values().any(|(ty, _, _)| *ty == 0 || *ty == 1);
    let render = has_render.then(|| {
        let log = std::fs::read_to_string(dir.join("log.txt")).expect("log.txt");
        let hwrt_args = std::fs::read(dir.join("hwrt1.args")).expect("hwrt1.args");
        let hwrt = HwrtArgs::parse(&hwrt_args).expect("the dataset's arguments parse");
        let mut lists: BTreeMap<u64, FreelistArgs> = BTreeMap::new();
        let mut render_ctx = None;
        for line in log.lines() {
            if line.starts_with("freelist ") {
                let handle = field(line, "handle=").expect("freelist handle");
                lists.insert(
                    handle,
                    FreelistArgs {
                        gpu_addr: field(line, "gpu_addr=").expect("gpu_addr"),
                        initial_pages: field(line, "initial=").expect("initial") as u32,
                        max_pages: field(line, "max=").expect("max") as u32,
                        grow_pages: field(line, "grow=").expect("grow") as u32,
                        grow_threshold_percent: field(line, "threshold=").expect("threshold")
                            as u32,
                    },
                );
            } else if line.starts_with("context ") && field(line, "type=") == Some(0) {
                render_ctx = Some((
                    field(line, "handle=").expect("handle"),
                    field(line, "callstack=").expect("callstack"),
                ));
            }
        }
        let freelists = [
            *lists.get(&u64::from(hwrt.free_list_handles[0])).expect("the local free list"),
            *lists.get(&u64::from(hwrt.free_list_handles[1])).expect("the global free list"),
        ];
        let (ctx, callstack) = render_ctx.expect("a render context in log.txt");
        let state = std::fs::read(dir.join(format!("context{ctx}.state")))
            .expect("the render context's state");
        (hwrt_args, callstack, freelists, state)
    });

    let mut b =
        Builder::new(CoreId::BOARD, if texture { Texture::CheckerV1 } else { Texture::None });
    for (va, len, kind, runs) in &buffers {
        b.buffer(*va, *len, *kind, runs);
    }
    for (n, (ty, flags, stream)) in &jobs {
        let kind = match (*ty, *flags & FRAG_PARTIAL_RENDER != 0) {
            (0, _) => JobKind::Geometry,
            (1, true) => JobKind::FragmentPartial,
            (1, false) => JobKind::Fragment,
            (3, _) => JobKind::Transfer,
            (t, _) => panic!("job {n}: type {t} is not packed in this layer"),
        };
        b.job(kind, *flags, stream);
    }
    if let Some((hwrt_args, callstack, freelists, state)) = &render {
        b.render(hwrt_args, *callstack, freelists, state);
    }
    let bytes = b.finish();
    let scene =
        Scene::parse(&bytes, CoreId::BOARD).expect("the bundle parses for the board's core");
    std::fs::write(out, &bytes).expect("write bundle");
    println!("| GPU address | Bytes | Kind | Runs | Carried |");
    println!("|---|---|---|---|---|");
    for r in rows {
        println!("{r}");
    }
    let kinds: Vec<String> = scene.jobs().map(|j| format!("{:?}/{:#x}", j.kind, j.flags)).collect();
    println!(
        "\n{} buffers, {} jobs [{}], {} data bytes carried, render trailer: {}, bundle {} bytes (FNV-1a 64 {:#018x}) -> {}",
        scene.buffer_count,
        scene.job_count,
        kinds.join(", "),
        scene.data_bytes(),
        scene.render().is_some(),
        bytes.len(),
        fnv1a64(&bytes),
        out.display()
    );
}
