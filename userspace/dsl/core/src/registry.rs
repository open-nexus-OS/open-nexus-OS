// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frontend's knowledge of the platform surface: widget kinds, the
//! modifier catalog (with field classes), interaction triggers, and the
//! read-only device environment.
//!
//! **SSOT note:** these const tables are the single source the checker, the
//! lowering pass, the runtime's widget registry generator, and the
//! `docs/dev/dsl/modifiers.md` catalog table derive from (docs emission via
//! `nx-dsl` — keeping frontend and runtime structurally unable to disagree).

/// Invalidation class of a modifier/property (docs/dev/dsl/ir.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldClass {
    Layout,
    Paint,
    Semantics,
}

/// Argument shape of a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModArg {
    /// Semantic token name (spacing step, color role, radius, …).
    Token,
    /// Plain integer (e.g. `.grow(1)`, `.truncate(2)`).
    Int,
    /// Boolean flag.
    Bool,
    /// Arbitrary expression (e.g. `.key(user.id)`).
    Expr,
    /// Translatable string (`.label(@t("…"))` or literal).
    Text,
}

pub struct ModifierSpec {
    pub name: &'static str,
    pub args: &'static [ModArg],
    pub class: FieldClass,
}

/// The modifier catalog (hybrid utility vocabulary, docs/dev/dsl/modifiers.md).
/// Order is the canonical catalog order; `modId` = index into this table.
pub const MODIFIERS: &[ModifierSpec] = &[
    // -- spacing (layout)
    ModifierSpec { name: "padding", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingX", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingY", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingTop", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingBottom", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingLeading", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingTrailing", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "gap", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "margin", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- sizing (layout)
    ModifierSpec { name: "width", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "height", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "minWidth", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "maxWidth", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "minHeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "maxHeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "grow", args: &[ModArg::Int], class: FieldClass::Layout },
    ModifierSpec { name: "shrink", args: &[ModArg::Int], class: FieldClass::Layout },
    ModifierSpec { name: "aspect", args: &[ModArg::Int, ModArg::Int], class: FieldClass::Layout },
    // -- layout
    ModifierSpec { name: "align", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "justify", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "direction", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "wrap", args: &[ModArg::Bool], class: FieldClass::Layout },
    ModifierSpec { name: "overflow", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "zIndex", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- color & surface (paint)
    ModifierSpec { name: "bg", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "fg", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "borderColor", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "opacity", args: &[ModArg::Int], class: FieldClass::Paint },
    ModifierSpec { name: "material", args: &[ModArg::Token], class: FieldClass::Paint },
    // -- shape & elevation (paint)
    ModifierSpec { name: "rounded", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "border", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "shadow", args: &[ModArg::Token], class: FieldClass::Paint },
    // -- typography (layout: metrics affect measurement)
    ModifierSpec { name: "textSize", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "fontWeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "textAlign", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "leading", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "truncate", args: &[ModArg::Int], class: FieldClass::Layout },
    // -- interaction
    ModifierSpec { name: "disabled", args: &[ModArg::Bool], class: FieldClass::Paint },
    ModifierSpec { name: "focusable", args: &[ModArg::Bool], class: FieldClass::Semantics },
    ModifierSpec { name: "hitSlop", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- accessibility (semantics)
    ModifierSpec { name: "label", args: &[ModArg::Text], class: FieldClass::Semantics },
    ModifierSpec { name: "role", args: &[ModArg::Token], class: FieldClass::Semantics },
    ModifierSpec { name: "hint", args: &[ModArg::Text], class: FieldClass::Semantics },
    // -- motion (paint)
    ModifierSpec { name: "animate", args: &[ModArg::Token, ModArg::Expr], class: FieldClass::Paint },
    ModifierSpec { name: "transition", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "effect", args: &[ModArg::Token, ModArg::Expr], class: FieldClass::Paint },
    // -- identity (layout)
    ModifierSpec { name: "key", args: &[ModArg::Expr], class: FieldClass::Layout },
];

#[must_use]
pub fn modifier_spec(name: &str) -> Option<(u16, &'static ModifierSpec)> {
    MODIFIERS
        .iter()
        .enumerate()
        .find(|(_, spec)| spec.name == name)
        .map(|(idx, spec)| (idx as u16, spec))
}

pub struct WidgetSpec {
    pub name: &'static str,
    /// Prop the positional sugar fills (`Text("hi")` → `value`).
    pub primary_prop: Option<&'static str>,
    /// Interactive nodes need an accessible name (label prop or `.label()`).
    pub interactive: bool,
    /// The prop that provides the accessible name if present.
    pub label_prop: Option<&'static str>,
    pub allows_children: bool,
}

/// v0.1 widget kinds (grows with the kit; the runtime registry is generated
/// from the same table).
pub const WIDGETS: &[WidgetSpec] = &[
    WidgetSpec { name: "Stack", primary_prop: None, interactive: false, label_prop: None, allows_children: true },
    WidgetSpec { name: "Spacer", primary_prop: None, interactive: false, label_prop: None, allows_children: false },
    WidgetSpec { name: "Text", primary_prop: Some("value"), interactive: false, label_prop: Some("value"), allows_children: false },
    WidgetSpec { name: "Icon", primary_prop: Some("symbol"), interactive: false, label_prop: None, allows_children: false },
    WidgetSpec { name: "Image", primary_prop: Some("source"), interactive: false, label_prop: None, allows_children: false },
    WidgetSpec { name: "Button", primary_prop: Some("label"), interactive: true, label_prop: Some("label"), allows_children: true },
    WidgetSpec { name: "Card", primary_prop: None, interactive: false, label_prop: None, allows_children: true },
    WidgetSpec { name: "TextField", primary_prop: Some("value"), interactive: true, label_prop: Some("label"), allows_children: false },
    WidgetSpec { name: "Toggle", primary_prop: Some("checked"), interactive: true, label_prop: Some("label"), allows_children: false },
    WidgetSpec { name: "List", primary_prop: None, interactive: false, label_prop: None, allows_children: true },
    WidgetSpec { name: "NativeWidget", primary_prop: None, interactive: false, label_prop: None, allows_children: false },
];

#[must_use]
pub fn widget_spec(name: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|spec| spec.name == name)
}

/// Interaction triggers handlers may bind (`on Tap -> …`).
pub const TRIGGERS: &[&str] = &["Tap", "Change", "Submit", "Focus", "Blur", "LongPress"];

/// Read-only device environment fields (docs/dev/dsl/profiles.md) + their
/// value vocabulary where enum-like.
pub const DEVICE_FIELDS: &[(&str, &[&str])] = &[
    ("profile", &["phone", "tablet", "desktop", "tv", "auto", "foldable", "convertible"]),
    ("posture", &["flat", "half_fold", "tent", "book"]),
    ("orientation", &["portrait", "landscape"]),
    ("shellMode", &[]),
    ("sizeClass", &["compact", "regular", "wide"]),
    ("dpiClass", &["low", "normal", "high"]),
    ("input", &["touch", "mouse", "kbd", "remote", "rotary"]),
];

#[must_use]
pub fn device_field(name: &str) -> Option<&'static [&'static str]> {
    DEVICE_FIELDS.iter().find(|(field, _)| *field == name).map(|(_, values)| *values)
}

// ------------------------------------------------------------ svc surface
// GENERATED from the IDL SSOT (tools/nexus-idl/schemas/dsl_services.capnp):
// `SvcSig` + `SVC_SURFACE`. The checker's unknown-service/method/arity
// diagnostics derive from the same file the app-host routes against.
include!(concat!(env!("OUT_DIR"), "/svc_surface.rs"));

/// Result of looking up `svc.<service>.<method>` against the surface.
pub enum SvcLookup {
    Found(&'static SvcSig),
    UnknownService,
    UnknownMethod,
}

#[must_use]
pub fn svc_method(service: &str, method: &str) -> SvcLookup {
    let mut service_exists = false;
    for sig in SVC_SURFACE {
        if sig.service == service {
            service_exists = true;
            if sig.method == method {
                return SvcLookup::Found(sig);
            }
        }
    }
    if service_exists {
        SvcLookup::UnknownMethod
    } else {
        SvcLookup::UnknownService
    }
}
