// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board GPU's backend against the hand-assembled references (TASK-0215 5c-2 P3c): the
//! compositor's layer — texel, tinted, blended premultiplied — compiled from our IR gives the
//! reference programs byte for byte, fused (iterate-and-sample) and unfused (iterate, sample),
//! with the temporaries and pass type the kick and the draw state need; what the subset lacks
//! is refused by name.

use nexus_gfx::shader::ir::builder::{store_blended, Blend, Builder};
use nexus_gfx::shader::ir::{BinOp, Interp, Lod, Module, Op, Stage, TexDim, Type, UnOp};
use nexus_gfx::shader::rogue::alu::{op, sngl, tst, AluInstr};
use nexus_gfx::shader::rogue::asm::{shared, special};
use nexus_gfx::shader::rogue::codegen::{
    compile, CodegenError, Layout, Options, Program, MAX_CONSTANTS,
};
use nexus_gfx::shader::rogue::encode_program;
use nexus_gfx::shader::rogue::reference::{self, Reference};
use nexus_gfx::shader::rogue::{Group, Instr, Phase};

/// The layer: `texture(surface, uv)`, times the tint when `tinted`, through the epilog.
fn layer(tinted: bool, blend: Blend) -> Module {
    let mut b = Builder::new(Stage::Fragment);
    let uv_in = b.input(0, Type::VEC2, Interp::Perspective);
    let out = b.output(0, Type::VEC4);
    let tex = b.texture(0, TexDim::D2);
    let smp = b.sampler(1);
    b.push_bytes(16);
    let uv = b.load_input(uv_in);
    let texel = b.sample(tex, smp, uv, Lod::Implicit);
    let color = if tinted {
        let tint = b.load_push(0, Type::VEC4);
        b.binary(BinOp::Mul, texel, tint)
    } else {
        texel
    };
    store_blended(&mut b, out, color, blend);
    b.finish().expect("the layer validates")
}

fn compiled(m: &Module, fused: bool) -> Program {
    compile(m, Layout::BUNDLE, Options { fused_sample: fused }).expect("the layer compiles")
}

fn same(p: &Program, r: &Reference) {
    assert_eq!(
        encode_program(&p.groups).expect("encodes"),
        r.encode().expect("encodes"),
        "{}: the compiled program is the reference",
        r.name
    );
    assert_eq!((p.temps, p.pass_type), (r.temps, r.pass_type), "{}: kick and pass type", r.name);
}

#[test]
fn the_texel_compiles_to_the_opaque_references() {
    let m = layer(false, Blend::Replace);
    same(&compiled(&m, false), &reference::opaque_ditr().unwrap());
    same(&compiled(&m, true), &reference::opaque_itrsmp().unwrap());
}

#[test]
fn the_tinted_layer_compiles_to_the_tinted_references() {
    let m = layer(true, Blend::Replace);
    same(&compiled(&m, false), &reference::tinted().unwrap());
    same(&compiled(&m, true), &reference::tinted_itrsmp().unwrap());
}

#[test]
fn the_premultiplied_layer_compiles_to_the_premultiplied_references() {
    let m = layer(true, Blend::OverPremultiplied);
    same(&compiled(&m, false), &reference::premultiplied().unwrap());
    same(&compiled(&m, true), &reference::premultiplied_itrsmp().unwrap());
}

#[test]
fn test_reject_what_the_backend_lacks() {
    let unsupported =
        |r: Result<Program, CodegenError>| matches!(r, Err(CodegenError::Unsupported { .. }));
    // A fractional part.
    let mut b = Builder::new(Stage::Fragment);
    let out = b.output(0, Type::VEC4);
    b.push_bytes(16);
    let t = b.load_push(0, Type::VEC4);
    let f = b.unary(UnOp::Fract, t);
    b.store_output(out, f);
    assert!(unsupported(compile(
        &b.finish().unwrap(),
        Layout::BUNDLE,
        Options { fused_sample: true }
    )));
    // Another stage.
    let mut m = layer(false, Blend::Replace);
    m.interface.stage = Stage::Compute;
    m.body.retain(|i| !matches!(i.op, Op::Sample { .. } | Op::StoreOutput(..)));
    assert!(compile(&m, Layout::BUNDLE, Options { fused_sample: true }).is_err());
    // More constants than a pipeline's block holds: a sum of MAX_CONSTANTS + 1 distinct values
    // no constant register holds.
    let mut b = Builder::new(Stage::Fragment);
    let out = b.output(0, Type::VEC4);
    let mut acc = b.f32s(&[0.3]);
    for i in 0..MAX_CONSTANTS {
        let c = b.f32s(&[0.3 + 0.01 * (i + 1) as f32]);
        acc = b.binary(BinOp::Add, acc, c);
    }
    let v = b.splat(acc, 4);
    b.store_output(out, v);
    assert!(unsupported(compile(
        &b.finish().unwrap(),
        Layout::BUNDLE,
        Options { fused_sample: true }
    )));
}

/// A group's instructions as their forms, registers left out.
fn forms(g: &Group) -> Vec<String> {
    g.instrs
        .iter()
        .map(|(ph, i)| match i {
            Instr::Alu(AluInstr::Sngl { op, .. }) => format!("{ph:?} sngl {op}"),
            Instr::Alu(AluInstr::Fam { op, sat, s0neg, s0abs, s0flr, .. }) => {
                format!("{ph:?} fam {op} sat={sat} neg={s0neg} abs={s0abs} flr={s0flr}")
            }
            Instr::Alu(AluInstr::Fmad { .. }) => format!("{ph:?} fmad"),
            Instr::Alu(AluInstr::Tst { op, .. }) => format!("{ph:?} tst {op}"),
            Instr::Alu(AluInstr::Movc { .. }) => format!("{ph:?} movc"),
            Instr::Alu(AluInstr::Pck { .. }) => format!("{ph:?} pck"),
            other => format!("{ph:?} {other:?}"),
        })
        .collect()
}

fn reads(g: &Group, r: nexus_gfx::shader::rogue::io::Reg) -> bool {
    [g.lower, g.upper].iter().flatten().any(|s| s.regs.contains(&r))
}

#[test]
fn the_float_units_ops_compile_to_the_compilers_forms() {
    // The open compiler's arithmetic probe (`frag-ops.usc`), as our IR: min, max, the
    // single-source unit, a saturated product, |z − w|·0.5 + g. The compiler copies all four push
    // constants into temporaries first; the backend moves only an operand the second source
    // cannot read.
    let mut b = Builder::new(Stage::Fragment);
    let out = b.output(0, Type::VEC4);
    b.push_bytes(16);
    let t = b.load_push(0, Type::VEC4);
    let (x, y, z, w) = (b.extract(t, 0), b.extract(t, 1), b.extract(t, 2), b.extract(t, 3));
    let e = b.binary(BinOp::Min, x, y);
    let f = b.binary(BinOp::Max, z, w);
    let a = b.unary(UnOp::Rsqrt, x);
    let r = b.unary(UnOp::Rcp, y);
    let c = b.unary(UnOp::Exp2, z);
    let d = b.unary(UnOp::Log2, w);
    let xy = b.binary(BinOp::Mul, x, y);
    let g = b.unary(UnOp::Sat, xy);
    let zw = b.binary(BinOp::Sub, z, w);
    let ab = b.unary(UnOp::Abs, zw);
    let half = b.f32s(&[0.5]);
    let h = b.fma(ab, half, g);
    let (ar, cd, ef) =
        (b.binary(BinOp::Add, a, r), b.binary(BinOp::Add, c, d), b.binary(BinOp::Add, e, f));
    let v = b.construct(&[ar, cd, ef, h]);
    store_blended(&mut b, out, v, Blend::Replace);
    let p = compile(&b.finish().unwrap(), Layout::BUNDLE, Options { fused_sample: true })
        .expect("compiles");
    let all: Vec<Vec<String>> = p.groups.iter().map(forms).collect();
    let byp = format!("P1 sngl {}", sngl::BYP);
    let expect: Vec<Vec<String>> = vec![
        vec![
            "P2Mov movc".into(),
            format!("P2Tst tst {}", tst::L),
            byp.clone(),
            format!("P0 sngl {}", sngl::BYP),
        ],
        vec![
            "P2Mov movc".into(),
            format!("P2Tst tst {}", tst::G),
            byp,
            format!("P0 sngl {}", sngl::BYP),
        ],
        vec![format!("P0 sngl {}", sngl::RSQ)],
        vec![format!("P0 sngl {}", sngl::RCP)],
        vec![format!("P0 sngl {}", sngl::EXP)],
        vec![format!("P0 sngl {}", sngl::LOG)],
        // x·y: both factors push constants (shared registers), one moved into a temporary.
        vec![format!("P0 sngl {}", sngl::BYP)],
        vec![format!("P0 fam {} sat=false neg=false abs=false flr=false", op::FMUL)],
        vec![format!("P0 fam {} sat=true neg=false abs=false flr=false", op::FADD)],
        // z − w: the same, then the subtraction as −w + z.
        vec![format!("P0 sngl {}", sngl::BYP)],
        vec![format!("P0 fam {} sat=false neg=true abs=false flr=false", op::FADD)],
        vec![format!("P0 fam {} sat=false neg=false abs=true flr=false", op::FADD)],
        vec!["P0 fmad".into()],
    ];
    assert_eq!(&all[..expect.len()], &expect[..]);
    // The 0.5 is constant register 75, read where the compiler reads it.
    assert!(reads(&p.groups[12], special(75)), "fmad reads sc75");
    // The last group packs into the pixel output and ends the program.
    assert!(p.groups.last().is_some_and(|g| g.header.end));
    assert_eq!(p.groups.iter().filter(|g| matches!(g.instrs[0].0, Phase::P2Pck)).count(), 1);
}

/// `(t · t) · x` over `lanes` (`t` the push constants), the constant made right before its use as
/// the frontend makes it.
fn weighted(x: f32, lanes: u8) -> Program {
    let mut b = Builder::new(Stage::Fragment);
    let out = b.output(0, Type::VEC4);
    b.push_bytes(16);
    let t = b.load_push(0, Type::VEC4);
    let tt = b.binary(BinOp::Mul, t, t);
    let c = b.f32s(&[x]);
    let v = if lanes == 1 {
        let (p, y, z, w) = (b.extract(tt, 0), b.extract(tt, 1), b.extract(tt, 2), b.extract(tt, 3));
        let m = b.binary(BinOp::Mul, p, c);
        b.construct(&[m, y, z, w])
    } else {
        let k = b.splat(c, 4);
        b.binary(BinOp::Mul, tt, k)
    };
    store_blended(&mut b, out, v, Blend::Replace);
    compile(&b.finish().unwrap(), Layout::BUNDLE, Options { fused_sample: true }).expect("compiles")
}

#[test]
fn constants_come_from_the_constant_registers_else_the_pipelines_block() {
    // A scalar 2.0 is constant register 65, read as it is.
    let two = weighted(2.0, 1);
    assert!(two.constants.is_empty());
    assert!(two.groups.iter().any(|g| reads(g, special(65))));
    // -0.5 (0.5's register only through the negate modifier) and 0.7 (no register) are the
    // constant block's — its first register after the push constants (24 + 4) — with no
    // instruction of their own: the groups 2.0 takes.
    let block = Layout::BUNDLE.constants(16);
    assert_eq!(block, 28);
    for x in [-0.5f32, 0.7] {
        let p = weighted(x, 1);
        assert_eq!(p.constants, [x.to_bits()]);
        assert_eq!(p.groups.len(), two.groups.len(), "{x}");
        assert!(p.groups.iter().any(|g| reads(g, shared(block))), "{x}");
    }
}

#[test]
fn a_broadcast_constant_is_a_run_of_the_block_and_its_group_repeats() {
    // Over four lanes the constant is four copies in the block — 0.7's scalar register extended,
    // 2.0 copied out of its constant register — and the multiply one repeated group reading them,
    // where a broadcast takes a group a lane: t · t (one push factor moved, one multiply), the
    // weighted product, the pack.
    let block = Layout::BUNDLE.constants(16);
    for x in [0.7f32, 2.0] {
        let p = weighted(x, 4);
        assert_eq!(p.constants, [x.to_bits(); 4], "{x}");
        assert_eq!(p.groups.iter().filter(|g| reads(g, shared(block))).count(), 1, "{x}");
        assert_eq!(p.groups.len(), 4, "{x}");
    }
}

#[test]
fn the_block_holds_each_run_once_in_the_order_the_program_reads_them() {
    // fma(t · t, 0.7, 0.3), then times 0.7 again: the two runs once each, the second 0.7 the
    // first's run.
    let mut b = Builder::new(Stage::Fragment);
    let out = b.output(0, Type::VEC4);
    b.push_bytes(48);
    let t = b.load_push(0, Type::VEC4);
    let tt = b.binary(BinOp::Mul, t, t);
    let k7 = b.f32s(&[0.7]);
    let s7 = b.splat(k7, 4);
    let k3 = b.f32s(&[0.3]);
    let s3 = b.splat(k3, 4);
    let a = b.fma(tt, s7, s3);
    let again = b.f32s(&[0.7]);
    let s7b = b.splat(again, 4);
    let v = b.binary(BinOp::Mul, a, s7b);
    store_blended(&mut b, out, v, Blend::Replace);
    let p = compile(&b.finish().unwrap(), Layout::BUNDLE, Options { fused_sample: true })
        .expect("compiles");
    // 0.7 then 0.3 were made before the multiply-add read either: 0.7's run lands behind 0.3.
    let (s, h) = (0.7f32.to_bits(), 0.3f32.to_bits());
    assert_eq!(p.constants, [s, h, s, s, s, s, h, h, h, h]);
    // Twelve dwords of push constants: the block from 24 + 12 = 36; the two products read the
    // 0.7 run, the multiply-add the 0.3 run too.
    let block = Layout::BUNDLE.constants(48);
    assert_eq!(block, 36);
    assert_eq!(p.groups.iter().filter(|g| reads(g, shared(block + 2))).count(), 2);
    assert_eq!(p.groups.iter().filter(|g| reads(g, shared(block + 6))).count(), 1);
}

#[test]
fn the_straight_alpha_epilog_compiles_its_vector_in_place() {
    // The tinted layer blended straight: the result vector takes the colour's registers and moves
    // only its alpha in behind them.
    let p = compiled(&layer(true, Blend::OverStraight), true);
    assert_eq!(p.pass_type, reference::PASS_TRANSLUCENT);
    let moves = p
        .groups
        .iter()
        .filter(|g| forms(g) == vec![format!("P0 sngl {}", sngl::BYP)] && !g.header.olchk)
        .count();
    assert_eq!(moves, 1, "one move: the alpha behind the colour");
}
