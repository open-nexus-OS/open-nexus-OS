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
use nexus_layout_types::{Align, Direction, EdgeInsets, FxPx, Justify};

/// Applies one modifier; mod ids index the compiler catalog
/// (`nexus-dsl-core::registry::MODIFIERS`) — matched here by stable id order.
pub(super) fn apply_modifier(
    ctx: &mut EmitCtx<'_, '_>,
    modifier: ir::modifier::Reader<'_>,
    mods: &mut Mods,
) -> Result<(), RtError> {
    let args = modifier.get_args().map_err(|_| RtError::Malformed)?;
    let first = args.iter().next();
    let token_name = |ctx: &EmitCtx<'_, '_>| -> String {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Token(sym))) => String::from(ctx.symbol(sym)),
            _ => String::new(),
        }
    };
    // An Int argument (`.columns(3)`, `.grow(2)`, `.opacity(128)` …), or an Int expression
    // (`.columns($props.columns)`, `.grow($state.share)`) — a LAYOUT dependency (each of these
    // shapes the layout or the opacity it carries). As with `px_arg`: the checker accepts the
    // expression form, so reading it as 0 was a silent no-op.
    let int_arg = |ctx: &mut EmitCtx<'_, '_>| -> Result<i64, RtError> {
        Ok(match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => i,
            Some(Ok(ir::token_arg::Which::Expr(Ok(expr)))) => {
                ctx.record_deps(expr, Damage::Layout);
                match ctx.eval(expr)? {
                    Value::Int(i) => i,
                    _ => 0,
                }
            }
            _ => 0,
        })
    };
    // Raw-px size argument (`.width(320)`), or an Int expression (`.width($state.selW)`, RFC-0095:
    // geometry that follows the store — a LAYOUT dependency, so the node re-lays out when the
    // value changes); a token (`full`) or a non-Int value yields None. The checker always
    // accepted the expression form; ignoring it here was a silent no-op.
    let px_arg = |ctx: &mut EmitCtx<'_, '_>| -> Result<Option<FxPx>, RtError> {
        let px = |i: i64| FxPx::new(i.clamp(0, 16384) as i32);
        Ok(match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => Some(px(i)),
            Some(Ok(ir::token_arg::Which::Expr(Ok(expr)))) => {
                ctx.record_deps(expr, Damage::Layout);
                match ctx.eval(expr)? {
                    Value::Int(i) => Some(px(i)),
                    _ => None,
                }
            }
            _ => None,
        })
    };
    // A spacing (RFC-0116): px on the 4-pt grid, a spacing name (`gapInline` …), or an Int
    // expression — a LAYOUT dependency.
    let spacing_arg = |ctx: &mut EmitCtx<'_, '_>| -> Result<FxPx, RtError> {
        Ok(match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => registry::spacing(i),
            Some(Ok(ir::token_arg::Which::Token(sym))) => {
                registry::spacing_token(ctx.symbol(sym)).unwrap_or(FxPx::ZERO)
            }
            Some(Ok(ir::token_arg::Which::Expr(Ok(expr)))) => {
                ctx.record_deps(expr, Damage::Layout);
                match ctx.eval(expr)? {
                    Value::Int(i) => registry::spacing(i),
                    _ => FxPx::ZERO,
                }
            }
            _ => FxPx::ZERO,
        })
    };
    // Catalog order (docs/dev/dsl/modifiers.md); ids are stable.
    match modifier.get_mod_id() {
        0 => mods.padding = EdgeInsets::all(spacing_arg(ctx)?), // padding
        1 => {
            let px = spacing_arg(ctx)?;
            mods.padding.left = px;
            mods.padding.right = px;
        } // paddingX
        2 => {
            let px = spacing_arg(ctx)?;
            mods.padding.top = px;
            mods.padding.bottom = px;
        } // paddingY
        3 => mods.padding.top = spacing_arg(ctx)?,              // paddingTop
        4 => mods.padding.bottom = spacing_arg(ctx)?,           // paddingBottom
        5 => mods.padding.left = spacing_arg(ctx)?,             // paddingLeading
        6 => mods.padding.right = spacing_arg(ctx)?,            // paddingTrailing
        7 => mods.gap = spacing_arg(ctx)?,                      // gap
        // Sizes are RAW px (modifiers.md: "length token | full | Int px");
        // `full` stays a no-op (cross-axis children stretch by default).
        9 => mods.width = px_arg(ctx)?,                // width
        10 => mods.height = px_arg(ctx)?,              // height
        11 => mods.min_width = px_arg(ctx)?,           // minWidth
        12 => mods.max_width = px_arg(ctx)?,           // maxWidth
        13 => mods.min_height = px_arg(ctx)?,          // minHeight
        14 => mods.max_height = px_arg(ctx)?,          // maxHeight
        15 => mods.grow = int_arg(ctx)?.max(0) as u32, // grow
        16 => mods.shrink = Some(int_arg(ctx)?.max(0) as u32), // shrink
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
        24 => mods.bg = color_arg(ctx, args, 0)?,      // bg
        25 => mods.fg = color_arg(ctx, args, 0)?,      // fg
        27 => mods.opacity = Some(int_arg(ctx)?.clamp(0, 255) as u8), // opacity
        28 => mods.material = registry::material(&token_name(ctx)), // material
        // Corner radius: a token from the scale (sm 8 · md 12 · lg 16 · xl 24 · full), or raw
        // px — a computed concentric radius (parent − padding) has no token of its own.
        29 => mods.rounded = Some(radius_at(ctx, args, 0)), // rounded
        31 => {
            // shadow(a, b?): an elevation and/or the well or the lit top edge.
            for (slot, i) in [(0usize, 0u32), (1, 1)] {
                mods.shadows[slot] = token_at(ctx, args, i).and_then(|n| registry::shadow(&n));
            }
        } // shadow
        33 => mods.font_weight = registry::font_weight(&token_name(ctx)), // fontWeight
        34 => mods.text_align = registry::text_align(&token_name(ctx)), // textAlign
        36 => mods.max_lines = Some(int_arg(ctx)?.clamp(1, 64) as u32), // truncate(n)
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
        39 => mods.hit_slop = spacing_arg(ctx)?,            // hitSlop
        22 => {
            // overflow(visible|hidden) — hidden = a clipping container (never a scroll layer).
            mods.clip = token_name(ctx).as_str() == "hidden";
        } // overflow
        47 => {
            // scroll(vertical|horizontal|paged): the page's scroll viewport.
            // `paged` = horizontal + host-side page snap (launcher pager).
            mods.scroll = Some(match token_name(ctx).as_str() {
                "horizontal" => registry::ScrollAxis::Horizontal,
                "paged" => registry::ScrollAxis::Paged,
                _ => registry::ScrollAxis::Vertical,
            });
        } // scroll
        // overlay(kind?): full-bleed out-of-flow layer; the kind is the modal
        // contract (TASK-0074 D1) — `.overlay(modal)` / `.overlay(transient)`.
        48 => {
            mods.overlay = Some(match token_name(ctx).as_str() {
                "modal" => crate::overlay::OverlayKind::Modal,
                "transient" => crate::overlay::OverlayKind::Transient,
                _ => crate::overlay::OverlayKind::Plain,
            });
        }
        // dismissAfter(ms): the transient layer's declared lifetime (host timer).
        56 => {
            mods.dismiss_after = Some(
                int_arg(ctx)?.clamp(0, i64::from(crate::overlay::DISMISS_AFTER_MAX_MS)) as u32,
            );
        }
        // autofocus(b): the host's text-focus target while no field holds focus (TASK-0067B).
        57 => {
            if let Some(Ok(ir::token_arg::Which::Boolean(b))) = first.map(|a| a.which()) {
                mods.autofocus = b;
            }
        }
        52 => mods.columns = Some(int_arg(ctx)?.clamp(1, 12) as usize), // columns(n): grid tracks
        53 => mods.row_gap = Some(spacing_arg(ctx)?),                   // rowGap(px)
        // basis(n): raw px like `.width`, NOT a spacing step — it names a
        // geometry the layout divides, not a design-system rhythm.
        54 => mods.basis = Some(FxPx::new(int_arg(ctx)?.clamp(0, 16384) as i32)), // basis(n)
        55 => {
            // textFit(pct, min, max): three positional ints, read by INDEX —
            // `first` only ever sees the leading argument (the `.bgFade` shape).
            let nth = |n: usize| -> i64 {
                match args.get(n as u32).which() {
                    Ok(ir::token_arg::Which::Int(i)) => i,
                    _ => 0,
                }
            };
            let px = |v: i64| FxPx::new(v.clamp(0, 16384) as i32);
            mods.text_fit = Some(nexus_layout_types::TextFit {
                pct: nth(0).clamp(1, 100) as u32,
                min: px(nth(1)),
                max: px(nth(2)),
            });
        } // textFit
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
        26 => mods.border_color = color_arg(ctx, args, 0)?,                       // borderColor
        30 => mods.border_width = registry::border_width(&token_name(ctx)),       // border
        // 32 textSize, 35 leading, 50 textShadow: retired by RFC-0116 — the checker rejects
        // them, so no arm (their rows stay in the catalog only because ids are positional).
        58 => mods.text_style = registry::text_style(&token_name(ctx)), // textStyle
        59 => {
            // tint(role, level?): the role at a wash strength as the background.
            if let Some(role) = color_arg(ctx, args, 0)? {
                let level = token_at(ctx, args, 1).unwrap_or_default();
                mods.tint = Some((role, registry::tint_alpha(&level)));
            }
        } // tint
        60 => mods.border_tint = color_arg(ctx, args, 0)?,              // borderTint
        62 => mods.figures = registry::figures(&token_name(ctx)),       // figures
        61 => {
            // corners(tl, tr, br, bl): per-corner radii (a bubble's tail).
            mods.corners = Some(nexus_layout_types::CornerRadius {
                top_left: radius_at(ctx, args, 0),
                top_right: radius_at(ctx, args, 1),
                bottom_right: radius_at(ctx, args, 2),
                bottom_left: radius_at(ctx, args, 3),
            });
        } // corners
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

/// The token name of argument `i`, if it is a token.
fn token_at(ctx: &EmitCtx<'_, '_>, args: ArgList<'_>, i: u32) -> Option<String> {
    let arg = (args.len() > i).then(|| args.get(i))?;
    match arg.which() {
        Ok(ir::token_arg::Which::Token(sym)) => Some(String::from(ctx.symbol(sym))),
        _ => None,
    }
}

/// A colour role from argument `i`: a token, or an expression naming one (`.bg($props.tint)` —
/// a role that comes from data, RFC-0116; a PAINT dependency). An unknown name paints nothing.
fn color_arg(
    ctx: &mut EmitCtx<'_, '_>,
    args: ArgList<'_>,
    i: u32,
) -> Result<Option<nexus_theme_tokens::ColorToken>, RtError> {
    let Some(arg) = (args.len() > i).then(|| args.get(i)) else { return Ok(None) };
    Ok(match arg.which() {
        Ok(ir::token_arg::Which::Token(sym)) => registry::color_token(ctx.symbol(sym)),
        Ok(ir::token_arg::Which::Expr(Ok(expr))) => {
            ctx.record_deps(expr, Damage::Paint);
            match ctx.eval(expr)? {
                Value::Str(name) => registry::color_token(&name),
                _ => None,
            }
        }
        _ => None,
    })
}

/// A radius from argument `i`: a radius token or raw px (0..=9999); anything else is 0.
fn radius_at(ctx: &EmitCtx<'_, '_>, args: ArgList<'_>, i: u32) -> FxPx {
    let Some(arg) = (args.len() > i).then(|| args.get(i)) else { return FxPx::ZERO };
    match arg.which() {
        Ok(ir::token_arg::Which::Int(px)) => FxPx::new(px.clamp(0, 9999) as i32),
        Ok(ir::token_arg::Which::Token(sym)) => {
            registry::radius(ctx.symbol(sym)).unwrap_or(FxPx::ZERO)
        }
        _ => FxPx::ZERO,
    }
}

/// The argument list of one modifier.
type ArgList<'a> = capnp::struct_list::Reader<'a, ir::token_arg::Owned>;

/// Parses `#rrggbb` / `#rrggbbaa` into RGBA bytes (alpha defaults to 255).
fn parse_hex_color(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([b(0)?, b(2)?, b(4)?, if hex.len() == 8 { b(6)? } else { 255 }])
}
