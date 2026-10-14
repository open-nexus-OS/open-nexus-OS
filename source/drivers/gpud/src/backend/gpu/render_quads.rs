// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic geometry on the board (TASK-0215 5b, RFC-0101 §8d): textured, tinted quads at
//! positions the device chooses, drawn with the captured pass's programs through the bindings
//! and render objects the render rung left. Every pass lands in fresh memory the GPU has not
//! read (`nexus_gfx::backend::gpu::quads`): the control stream, the vertices, the vertex PDS
//! data, each draw's blocks; the geometry job's `vdm_ctrl_stream_base` is patched to the pass's
//! stream. The target is rebound at a fresh plane once; a sample inside each quad is read back
//! against the formula (texel × tint over the clear). Probes, each a line, keep the diagnosis
//! sharp: one quad (data 1), one again (data 0), two, three (the gate), two with the fragment
//! programs shared: `gpud: gpu quads ok (n=3 us= samples=3/3)`.

use nexus_driverkit::gpu::{Access, GpuVa};
use nexus_gfx::backend::gpu::csb::ppp;
use nexus_gfx::backend::gpu::quads::{self, Bump, Placer, Quad, Template};
use nexus_gfx::backend::gpu::scene::{JobKind, Scene};
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{
    Batch, ClientId, ContextId, GfxDevice, QueueKind, RenderStreams, RenderTarget, Stream,
    TimelinePoint,
};

use super::device_v2::Device;
use super::emit;
use crate::backend::display::{Display, GpuCanvas};

const BUNDLE: &[u8] = include_bytes!("../../../../../../resources/gpu/scenes/render-quad.scene");
const PASS_TIMEOUT_NS: u64 = 2_000_000_000;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const STRIDE: usize = 7680;
const TOLERANCE: u8 = 4;

/// The captured pass's arenas (`docs/board/measurements/2026-10-13-gpu-render-queue/README.md`, 5b).
const GENERAL_ARENA: u64 = 0x80_0004_b000;
const GENERAL_LEN: u32 = 128 * 1024;
const PDS_ARENA: u64 = 0xda_0000_1000;
const PDS_HEAP_BASE: u64 = 0xda_0000_0000;
const PDS_LEN_HEAP: u32 = 0x21000;
const VERTEX_BUFFER: u64 = 0x80_008d_a000;
const VERTEX_LEN: u32 = 4096;
const STREAM_BUFFER: u64 = 0x80_010d_1000;
const STREAM_LEN: u32 = 4096;
/// The captured first draw's blocks: the PDS heap offsets of the vertex PDS data, the fragment
/// uniform data (its three patched descriptors at bytes 8, 16, 24), the shader and varying
/// programs; the general arena offsets of the 26-word PPP block and the constant word.
const VS_PDS_DATA_HEAP: u64 = 0x2840;
const UNIFORM_DATA_HEAP: u64 = 0x2880;
const SHADER_HEAP: u64 = 0x2900;
const VARYING_HEAP: u64 = 0x2940;
const PPP26_OFFSET: u32 = 0x8200;
const CONSTANT_BLOCK_OFFSET: u32 = 0x4180;
const TERMINATE_PPP: (u64, u32) = (GENERAL_ARENA + 0x40, 3);
/// Where the geometry job stream carries `vdm_ctrl_stream_base` (after the 8-byte header).
const GEOM_STREAM_BASE_AT: usize = 8;
/// The three quads: the capture's positions and tints.
const QUADS: [Quad; 3] = [
    Quad { x: 704.0, y: 284.0, w: 512.0, h: 512.0, tint: [1.0, 1.0, 1.0, 1.0] },
    Quad { x: 304.0, y: 134.0, w: 512.0, h: 512.0, tint: [1.0, 0.5, 0.5, 1.0] },
    Quad { x: 1104.0, y: 434.0, w: 512.0, h: 512.0, tint: [0.5, 0.5, 1.0, 0.6] },
];
/// The sample: 272 px into the quad on both axes — texel (136, 136), inside checker block (4, 4)
/// (the quad's centre sits on a block boundary, where the bilinear filter mixes two colours).
const SAMPLE_INSET: f32 = 272.0;
/// The clear colour in memory order (B, G, R) and the sampled texel (R, G, B, A): colour
/// (40, 90, 200), alpha (136 + 136) × 255 / 510 = 136.
const CLEAR_BGR: [f32; 3] = [41.0, 31.0, 26.0];
const SAMPLE_TEXEL: [f32; 4] = [40.0, 90.0, 200.0, 136.0];

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// The expected memory bytes (B, G, R) at a quad's sample: source-over of texel × tint.
fn expected(q: &Quad) -> [u8; 3] {
    let a = SAMPLE_TEXEL[3] / 255.0 * q.tint[3];
    let rgb =
        [SAMPLE_TEXEL[0] * q.tint[0], SAMPLE_TEXEL[1] * q.tint[1], SAMPLE_TEXEL[2] * q.tint[2]];
    let mix = |src: f32, dst: f32| (src * a + dst * (1.0 - a) + 0.5) as u8;
    [mix(rgb[2], CLEAR_BGR[0]), mix(rgb[1], CLEAR_BGR[1]), mix(rgb[0], CLEAR_BGR[2])]
}

/// Copies `bytes` into the bound buffer at `va + at` and cleans it.
fn write_bound(
    dev: &mut Device,
    client: ClientId,
    va: u64,
    at: u32,
    bytes: &[u8],
) -> Result<(), alloc::string::String> {
    let bo = dev
        .bo_at(va, u64::from(at) + bytes.len() as u64)
        .ok_or_else(|| alloc::format!("{va:#x}+{at:#x} not bound"))?;
    let view = dev.bo_bytes_mut(client, bo).ok_or("no cpu view")?;
    let dst =
        view.get_mut(at as usize..at as usize + bytes.len()).ok_or("write past the buffer")?;
    dst.copy_from_slice(bytes);
    dev.clean(client, bo);
    Ok(())
}

fn read_bound(
    dev: &mut Device,
    client: ClientId,
    va: u64,
    at: u32,
    len: usize,
) -> Option<alloc::vec::Vec<u8>> {
    let bo = dev.bo_at(va, u64::from(at) + len as u64)?;
    let view = dev.bo_bytes_mut(client, bo)?;
    view.get(at as usize..at as usize + len).map(|s| s.to_vec())
}

fn heap_to_bo(heap: u64) -> u32 {
    (heap - (PDS_ARENA - PDS_HEAP_BASE)) as u32
}

fn words(bytes: &[u8]) -> alloc::vec::Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// One pass of `quads` placed by `place`: the writes, the kick, the samples. `Ok((us, irqs))`.
#[allow(clippy::too_many_arguments)]
fn probe(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    client: ClientId,
    ctx: ContextId,
    t: &Template<'_>,
    streams: &RenderStreams<'_>,
    target_va: u64,
    quads: &[Quad],
    place: &mut dyn Placer,
    share_programs: bool,
) -> Result<(u64, u32), alloc::string::String> {
    let emitted =
        quads::emit(t, quads, place, share_programs).map_err(|e| alloc::format!("emit {e:?}"))?;
    let mut writes: alloc::vec::Vec<(u64, u32, &[u8])> = alloc::vec::Vec::new();
    writes.push((VERTEX_BUFFER, emitted.vertex_offset, &emitted.vertices));
    writes.push((STREAM_BUFFER, emitted.stream_offset, &emitted.stream));
    for w in &emitted.general {
        writes.push((GENERAL_ARENA, w.offset, &w.bytes));
    }
    for w in &emitted.pds {
        writes.push((PDS_ARENA, heap_to_bo(u64::from(w.offset)), &w.bytes));
    }
    for (va, at, bytes) in writes {
        write_bound(dev, client, va, at, bytes)
            .map_err(|e| alloc::format!("write {va:#x}+{at:#x}: {e}"))?;
    }
    if canvas.block != 0 {
        if let Some(bytes) = display.gpu_canvas_bytes(canvas.row) {
            nexus_abi::cache_clean(bytes, canvas.block);
        }
    }
    // The geometry job names the pass's stream.
    let mut geom = streams.geom.to_vec();
    if geom.len() < GEOM_STREAM_BASE_AT + 8 {
        return Err(alloc::string::String::from("the geometry stream is too short to patch"));
    }
    geom[GEOM_STREAM_BASE_AT..GEOM_STREAM_BASE_AT + 8]
        .copy_from_slice(&emitted.vdm_ctrl_stream_base.to_le_bytes());
    let pass = RenderStreams { geom: &geom, ..*streams };
    let batch = Batch {
        target: RenderTarget { va: GpuVa(target_va), width: WIDTH, height: HEIGHT },
        stream: Stream::Render(pass),
        waits: &[],
        signal: TimelinePoint(0),
    };
    let irqs_before = dev.gpu.irqs;
    let t0 = now_ns();
    let point = dev
        .submit(client, ctx, QueueKind::Render, &batch)
        .map_err(|e| alloc::format!("submit {e:?}"))?;
    if !dev.wait_point(point, t0 + PASS_TIMEOUT_NS) {
        dev.gpu.drain_fwccb();
        let (gw, fw) = dev.render_words().unwrap_or((0, 0));
        return Err(alloc::format!(
            "pass not done in {}us (point {:#x} geom_word={gw} frag_word={fw} irqs={})",
            now_ns().saturating_sub(t0) / 1000,
            point.0,
            dev.gpu.irqs - irqs_before
        ));
    }
    let us = now_ns().saturating_sub(t0) / 1000;
    let irqs = dev.gpu.irqs - irqs_before;
    dev.gpu.drain_fwccb();
    let bytes = display.gpu_canvas_bytes(canvas.row).ok_or("no cpu view of the plane")?;
    if canvas.block != 0 {
        nexus_abi::cache_flush(bytes, canvas.block);
    }
    let mut good = 0;
    let mut bad = None;
    for (i, q) in quads.iter().enumerate() {
        let (x, y) = ((q.x + SAMPLE_INSET) as usize, (q.y + SAMPLE_INSET) as usize);
        let want = expected(q);
        let at = y * STRIDE + x * 4;
        let Some(px) = bytes.get(at..at + 3) else {
            bad.get_or_insert((i, x, y, want, [0u8; 3]));
            continue;
        };
        if px.iter().zip(&want).all(|(p, w)| p.abs_diff(*w) <= TOLERANCE) {
            good += 1;
        } else {
            bad.get_or_insert((i, x, y, want, [px[0], px[1], px[2]]));
        }
    }
    match bad {
        None => Ok((us, irqs)),
        Some((i, x, y, want, got)) => Err(alloc::format!(
            "quad {i} sample ({x},{y}) expected bgr {:02x}{:02x}{:02x} read {:02x}{:02x}{:02x}; {good}/{} us={us} irqs={irqs}",
            want[0],
            want[1],
            want[2],
            got[0],
            got[1],
            got[2],
            quads.len()
        )),
    }
}

/// The rung: after `gpu render ok`, with the render rung's bindings and objects.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, display: &mut dyn Display) {
    let fail = |what: &str| emit(&alloc::format!("gpud: FAIL gpu quads ({what})"));
    let core = CoreId(dev.gpu.win.core_id());
    let scene = match Scene::parse(BUNDLE, core) {
        Ok(s) => s,
        Err(e) => return fail(&alloc::format!("scene refused {e:?}")),
    };
    let Some(target) = scene.target() else {
        return fail("scene without a target");
    };
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    let ctx = ContextId(1);
    // The template, from the live arenas the render rung bound and filled.
    let Some(stream0) = read_bound(dev, client, STREAM_BUFFER, 0, 0x30) else {
        return fail("the stream buffer is not bound (the render rung did not run)");
    };
    let (Some(vs), Some(uniform), Some(shader), Some(varying)) = (
        read_bound(dev, client, PDS_ARENA, heap_to_bo(VS_PDS_DATA_HEAP), 64),
        read_bound(dev, client, PDS_ARENA, heap_to_bo(UNIFORM_DATA_HEAP), 48),
        read_bound(dev, client, PDS_ARENA, heap_to_bo(SHADER_HEAP), 64),
        read_bound(dev, client, PDS_ARENA, heap_to_bo(VARYING_HEAP), 64),
    ) else {
        return fail("the PDS arena is not bound");
    };
    let (Some(ppp26), Some(constant)) = (
        read_bound(dev, client, GENERAL_ARENA, PPP26_OFFSET, 104),
        read_bound(dev, client, GENERAL_ARENA, CONSTANT_BLOCK_OFFSET, 16),
    ) else {
        return fail("the general arena is not bound");
    };
    let w = words(&stream0);
    let template = Template {
        pds_state_words: [w[0], w[1], w[2]],
        vdm_state_words: [w[5], w[6], w[7], w[8], w[9]],
        ppp26: &ppp26,
        terminate_ppp: TERMINATE_PPP,
        vs_pds_data: &vs,
        vs_patched: [16, 48],
        uniform_data: &uniform,
        patched: [8, 16, 24],
        constant_block: &constant,
        shader_program: &shader,
        varying_program: &varying,
        general_va: GENERAL_ARENA,
        vertex_va: VERTEX_BUFFER,
        stream_va: STREAM_BUFFER,
        pds: ppp::PdsState {
            shaderbase: SHADER_HEAP as u32,
            texunicodebase: 0x2700,
            varyingbase: VARYING_HEAP as u32,
            uniformdatabase: UNIFORM_DATA_HEAP as u32,
            sizeinfo1: ppp::sizeinfo1(3, 0, 1, 1, 1),
            sizeinfo2: ppp::sizeinfo2(2, true, 0),
        },
        cam_size: 32,
        topology: 6,
    };
    let (mut geom, mut frag_pr, mut frag) = (None, None, None);
    for job in scene.jobs() {
        match job.kind {
            JobKind::Geometry => geom = Some(job),
            JobKind::FragmentPartial => frag_pr = Some(job),
            JobKind::Fragment => frag = Some(job),
            JobKind::Transfer => {}
        }
    }
    let (Some(g), Some(pr), Some(f)) = (geom, frag_pr, frag) else {
        return fail("the bundle lacks one of the three render jobs");
    };
    let streams = RenderStreams {
        geom: g.stream,
        geom_flags: g.flags,
        frag_pr: pr.stream,
        frag_pr_flags: pr.flags,
        frag: f.stream,
        frag_flags: f.flags,
    };
    // A fresh plane under the target address.
    let Some(canvas) = display.gpu_canvas() else {
        return fail("no plane of the ring free");
    };
    if let Err(e) = dev.vm_unbind(client, ctx, GpuVa(target.va)) {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("unbind target {e:?}"));
    }
    let rebind = dev
        .adopt_plane(client, canvas.bus, target.len as usize)
        .and_then(|bo| dev.vm_bind(client, ctx, bo, GpuVa(target.va), Access::ReadWrite));
    if let Err(e) = rebind {
        display.gpu_canvas_discard(canvas.row);
        return fail(&alloc::format!("rebind target {e:?}"));
    }
    // Fresh lines for every pass: the tails of the arenas, the stream and vertex buffers past
    // the captured pass's bytes.
    let mut place = Bump {
        general_free: 0x1_0000,
        general_len: GENERAL_LEN,
        pds_free: 0x3000,
        pds_len: PDS_LEN_HEAP,
        stream_free: 0x100,
        stream_len: STREAM_LEN,
        vertex_free: 0x200,
        vertex_len: VERTEX_LEN,
    };
    let probes: [(&str, &[Quad], bool); 5] = [
        ("one", &QUADS[..1], false),
        ("one-again", &QUADS[..1], false),
        ("two", &QUADS[..2], false),
        ("three", &QUADS[..3], false),
        ("two-shared", &QUADS[..2], true),
    ];
    let mut gate = Err(alloc::string::String::from("no probe ran"));
    for (name, quads, share) in probes {
        let r = probe(
            dev, display, &canvas, client, ctx, &template, &streams, target.va, quads, &mut place,
            share,
        );
        match &r {
            Ok((us, irqs)) => emit(&alloc::format!(
                "gpud: gpu quads probe {name} ok (n={} us={us} irqs={irqs})",
                quads.len()
            )),
            Err(e) => emit(&alloc::format!("gpud: gpu quads probe {name} FAIL ({e})")),
        }
        if name == "three" {
            gate = r;
        }
    }
    match gate {
        Ok((us, irqs)) => {
            if let Err(e) = display.gpu_canvas_show(canvas.row) {
                return fail(&alloc::format!("the plane could not be shown {e:?}"));
            }
            emit(&alloc::format!(
                "gpud: gpu quads ok (n=3 us={us} samples=3/3 irqs={irqs} shown row={})",
                canvas.row
            ));
        }
        Err(e) => {
            display.gpu_canvas_discard(canvas.row);
            fail(&e);
        }
    }
}
