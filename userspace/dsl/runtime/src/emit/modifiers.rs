// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Modifier application: one IR `modId` -> one field of [`Mods`].
//!
//! The match arms are keyed by the RAW id, which is the wire contract — the
//! index into `nexus_dsl_core::registry::MODIFIERS` (append-only). A missing
//! arm is a declared-but-unimplemented modifier; `docs/dev/dsl/modifiers.md`
//! lists which those are, because a silent no-op is worse than an error.

use super::{stamp_anim, AnimKind, Damage, EmitCtx};
use crate::registry::{self, Mods};
use crate::store::Value;
use crate::RtError;
use alloc::string::String;
use nexus_dsl_ir::ui_ir_capnp as ir;
use nexus_layout_types::{Align, Direction, EdgeInsets, Justify};

/// Applies one modifier; mod ids index the compiler catalog
/// (`nexus-dsl-core::registry::MODIFIERS`) — matched here by stable id order.
pub(super) fn apply_modifier(
    ctx: &mut EmitCtx<'_>,
    modifier: ir::modifier::Reader<'_>,
    mods: &mut Mods,
) -> Result<(), RtError> {
    let args = modifier.get_args().map_err(|_| RtError::Malformed)?;
    let first = args.iter().next();
    let token_name = |ctx: &EmitCtx<'_>| -> String {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Token(sym))) => String::from(ctx.symbol(sym)),
            _ => String::new(),
        }
    };
    let int_arg = || -> i64 {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => i,
            _ => 0,
        }
    };
    // Raw-px size argument (`.width(320)`); a token (`full`) yields None.
    let px_arg = || -> Option<nexus_layout_types::FxPx> {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => {
                Some(nexus_layout_types::FxPx::new(i.clamp(0, 16384) as i32))
            }
            _ => None,
        }
    };
    // Catalog order (docs/dev/dsl/modifiers.md); ids are stable.
    match modifier.get_mod_id() {
        0 => mods.padding = EdgeInsets::all(registry::spacing(int_arg())), // padding
        1 => {
            let px = registry::spacing(int_arg());
            mods.padding.left = px;
            mods.padding.right = px;
        } // paddingX
        2 => {
            let px = registry::spacing(int_arg());
            mods.padding.top = px;
            mods.padding.bottom = px;
        } // paddingY
        3 => mods.padding.top = registry::spacing(int_arg()),              // paddingTop
        4 => mods.padding.bottom = registry::spacing(int_arg()),           // paddingBottom
        5 => mods.padding.left = registry::spacing(int_arg()),             // paddingLeading
        6 => mods.padding.right = registry::spacing(int_arg()),            // paddingTrailing
        7 => mods.gap = registry::spacing(int_arg()),                      // gap
        // Sizes are RAW px (modifiers.md: "length token | full | Int px");
        // `full` stays a no-op (cross-axis children stretch by default).
        9 => mods.width = px_arg(),                        // width
        10 => mods.height = px_arg(),                      // height
        11 => mods.min_width = px_arg(),                   // minWidth
        12 => mods.max_width = px_arg(),                   // maxWidth
        13 => mods.min_height = px_arg(),                  // minHeight
        14 => mods.max_height = px_arg(),                  // maxHeight
        15 => mods.grow = int_arg().max(0) as u32,         // grow
        16 => mods.shrink = Some(int_arg().max(0) as u32), // shrink
        18 => {
            mods.align = Some(match token_name(ctx).as_str() {
                "start" => Align::Start,
                "center" => Align::Center,
                "end" => Align::End,
                _ => Align::Stretch,
            });
        } // align
        19 => {
            mods.justify = Some(match token_name(ctx).as_str() {
                "center" => Justify::Center,
                "end" => Justify::End,
                "between" => Justify::SpaceBetween,
                _ => Justify::Start,
            });
        } // justify
        20 => {
            mods.direction = Some(match token_name(ctx).as_str() {
                "row" => Direction::Row,
                _ => Direction::Column,
            });
        } // direction
        24 => mods.bg = registry::color_token(&token_name(ctx)), // bg
        25 => mods.fg = registry::color_token(&token_name(ctx)), // fg
        27 => mods.opacity = Some(int_arg().clamp(0, 255) as u8), // opacity
        28 => mods.material = registry::material_token(&token_name(ctx)), // material
        29 => mods.rounded = Some(registry::radius(&token_name(ctx))), // rounded
        31 => mods.shadow = registry::shadow_level(&token_name(ctx)), // shadow
        32 => mods.text_size = registry::type_size(&token_name(ctx)), // textSize
        33 => mods.font_weight = registry::font_weight(&token_name(ctx)), // fontWeight
        34 => mods.text_align = registry::text_align(&token_name(ctx)), // textAlign
        35 => mods.leading = registry::leading(&token_name(ctx)), // leading
        21 => {
            if let Some(Ok(ir::token_arg::Which::Boolean(b))) = first.map(|a| a.which()) {
                mods.wrap = b;
            }
        } // wrap
        37 => {
            // disabled(bool | expr) — PAINT-class dependency.
            if let Some(Ok(which)) = first.map(|a| a.which()) {
                match which {
                    ir::token_arg::Which::Boolean(b) => mods.disabled = b,
                    ir::token_arg::Which::Expr(Ok(expr)) => {
                        ctx.record_deps(expr, Damage::Paint);
                        mods.disabled = matches!(ctx.eval(expr)?, Value::Bool(true));
                    }
                    _ => {}
                }
            }
        } // disabled
        22 => {
            // overflow(visible|hidden) — hidden = clipped container.
            if token_name(ctx).as_str() == "hidden" {
                mods.scroll = mods.scroll.or(Some(registry::ScrollAxis::Vertical));
            }
        } // overflow
        47 => {
            // scroll(vertical|horizontal): the page's scroll viewport.
            mods.scroll = Some(match token_name(ctx).as_str() {
                "horizontal" => registry::ScrollAxis::Horizontal,
                _ => registry::ScrollAxis::Vertical,
            });
        } // scroll
        48 => mods.overlay = true, // overlay(): full-bleed out-of-flow layer
        49 => {
            // bgGradient(top, bottom): both args are exprs → "#rrggbb[aa]"
            // strings (literal or prop-fed). Unparseable = no gradient —
            // never a panic, the solid `.bg` (if any) stays in charge.
            let mut colors = [[0u8; 4]; 2];
            let mut ok = 0;
            for i in 0..2 {
                let Some(arg) = (args.len() > i as u32).then(|| args.get(i as u32)) else {
                    break;
                };
                if let Ok(ir::token_arg::Which::Expr(Ok(expr))) = arg.which() {
                    ctx.record_deps(expr, Damage::Paint);
                    if let Ok(Value::Str(text)) = ctx.eval(expr) {
                        if let Some(c) = parse_hex_color(&text) {
                            colors[i as usize] = c;
                            ok += 1;
                        }
                    }
                }
            }
            if ok == 2 {
                mods.bg_gradient = Some((colors[0], colors[1]));
            }
        } // bgGradient
        26 => mods.border_color = registry::color_token(&token_name(ctx)), // borderColor
        30 => mods.border_width = registry::border_width(&token_name(ctx)), // border
        50 => mods.text_shadow = registry::text_shadow(&token_name(ctx)), // textShadow
        51 => {
            // bgFade(top, bottom): the same vertical fill as `.bgGradient`,
            // but from two COLOR TOKENS so it re-themes. Both must resolve —
            // a half-resolved fade would paint a wrong color, so it is all or
            // nothing (the solid `.bg`, if any, stays in charge).
            let token_at = |i: u32| -> Option<[u8; 4]> {
                let arg = (args.len() > i).then(|| args.get(i))?;
                let Ok(ir::token_arg::Which::Token(sym)) = arg.which() else {
                    return None;
                };
                let c = ctx.tokens.color(registry::color_token(ctx.symbol(sym))?);
                Some([c.r, c.g, c.b, c.a])
            };
            if let (Some(top), Some(bottom)) = (token_at(0), token_at(1)) {
                mods.bg_gradient = Some((top, bottom));
            }
        } // bgFade
        // -- motion (paint): the curated `.animate`/`.transition`/`.effect`
        //    (docs/dev/ui/foundations/animation.md). The runtime stays PURE —
        //    it stamps a value-typed INTENT (token id + committed snapshot of
        //    the driving value) onto this node's path; the HOST owns the clock
        //    and physics (`AnimationDriver`). The driving/trigger expr records
        //    a PAINT dep so a change re-emits and refreshes the snapshot.
        43 => stamp_anim(ctx, args, AnimKind::Animate), // animate(token, value:)
        44 => stamp_anim(ctx, args, AnimKind::Transition), // transition(token)
        45 => stamp_anim(ctx, args, AnimKind::Effect),  // effect(token, trigger:)
        _ => {} // key/label/others: identity/semantics — no paint effect here
    }
    Ok(())
}

/// Parses `#rrggbb` / `#rrggbbaa` into RGBA bytes (alpha defaults to 255).
fn parse_hex_color(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([b(0)?, b(2)?, b(4)?, if hex.len() == 8 { b(6)? } else { 255 }])
}
