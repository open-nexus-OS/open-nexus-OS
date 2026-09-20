// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the widget catalog — the SSOT for what a widget IS, split out of
//! `registry.rs` under the structure ratchet (TASK-0077B P2).
//!
//! `docs/dev/dsl/principles.md` §7 calls the widget set "a curated catalog with
//! a UNIFORM modifier surface — not an open plugin zoo". This table is that
//! catalog, and the uniformity has to come FROM it: every rule about widgets is
//! read off these fields rather than kept in a list beside them. The two-way
//! bind rule was the counter-example — a hand-kept list of four names in
//! lowering, which is how `SearchBar` was left out and how `TextArea`, a widget
//! this DSL does not have, stayed in.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable (append-only: a widget is added, never renamed)
//! TEST_COVERAGE: `the_bind_rule_follows_the_registry_not_a_list`,
//!   `a_bound_slider_writes_the_fraction_of_its_track` (dsl_conformance)

pub use nexus_dsl_ir::ui_ir_capnp::BindValue;

/// How a control's primary prop is written back: the interaction that edits it,
/// and how that interaction produces the new value.
///
/// One rule, not two fields that can drift: a trigger without a derivation
/// synthesizes a handler the runtime cannot execute, and a derivation without a
/// trigger is never reached. The lowering writes BOTH into the IR
/// (`Handler.bind` = `BindWrite { target, value }`, schema v1.8), so the runtime
/// executes the rule and never needs the widget kind.
pub struct BindRule {
    /// Interaction symbol that edits the primary prop (`Tap`, `Change`).
    pub trigger: &'static str,
    /// How that interaction produces the value.
    pub value: BindValue,
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
    /// How this control's primary prop is EDITED, when the prop is a value the
    /// user changes rather than a label the app supplies.
    ///
    /// This is what makes the two-way bind rule uniform (`docs/dev/dsl/ir.md`
    /// v1.2: *"auto-synthesized when an interactive kind's primary prop is
    /// `$state`-bound"*). It used to be a hand-kept list of four widget names
    /// in `lower/views.rs` — which is how `SearchBar` was left out and how
    /// `TextArea`, a widget that does not exist, stayed in it (TASK-0077B P2).
    ///
    /// `None` for an interactive control whose primary prop is a LABEL
    /// (`Button`, `Chip`, `Toast`, `Banner`, `ListItem`): there is nothing for
    /// the user to write back. Also `None` for `Select`, whose tap OPENS an
    /// app-owned option panel rather than producing a value — declaring a bind
    /// there would synthesize a handler nothing can ever fire.
    pub bind: Option<BindRule>,
}

/// v0.1 widget kinds (grows with the kit; the runtime registry is generated
/// from the same table).
pub const WIDGETS: &[WidgetSpec] = &[
    WidgetSpec {
        name: "Stack",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    WidgetSpec {
        name: "Spacer",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Text",
        primary_prop: Some("value"),
        interactive: false,
        label_prop: Some("value"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Icon",
        primary_prop: Some("symbol"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Image",
        primary_prop: Some("source"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Button",
        primary_prop: Some("label"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: true,
        bind: None,
    },
    WidgetSpec {
        name: "Card",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    WidgetSpec {
        name: "TextField",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
        bind: Some(BindRule { trigger: "Change", value: BindValue::Text }),
    },
    WidgetSpec {
        name: "Toggle",
        primary_prop: Some("checked"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
        bind: Some(BindRule { trigger: "Tap", value: BindValue::ToggleBool }),
    },
    WidgetSpec {
        name: "List",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    WidgetSpec {
        name: "NativeWidget",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    // Design-system kit exposure (TASK-0073/0074): each maps 1:1 onto its
    // `userspace/ui/widgets/*` builder in the runtime registry.
    WidgetSpec {
        name: "Badge",
        primary_prop: Some("label"),
        interactive: false,
        label_prop: Some("label"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Chip",
        primary_prop: Some("label"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Avatar",
        primary_prop: Some("initials"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Checkbox",
        primary_prop: Some("checked"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
        bind: Some(BindRule { trigger: "Tap", value: BindValue::ToggleBool }),
    },
    // The slider's value IS a percent of its own track: the kit builder makes
    // the widget's root node the track itself (zero padding, the fill and the
    // remainder as flex children weighted `value` vs `100 - value`), so the
    // point's position across the handler's own box is the value, with no
    // per-widget geometry constant. `Tap` because that is the pointer trigger
    // the surface delivers; a drag rides the same rule once one exists.
    WidgetSpec {
        name: "Slider",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: None,
        allows_children: false,
        bind: Some(BindRule { trigger: "Tap", value: BindValue::TrackFraction }),
    },
    WidgetSpec {
        name: "Spinner",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "ProgressBar",
        primary_prop: Some("value"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Toast",
        primary_prop: Some("message"),
        interactive: true,
        label_prop: Some("message"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Banner",
        primary_prop: Some("message"),
        interactive: true,
        label_prop: Some("title"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Skeleton",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "ListItem",
        primary_prop: Some("title"),
        interactive: true,
        label_prop: Some("title"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Toolbar",
        primary_prop: Some("title"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "SearchBar",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("placeholder"),
        allows_children: false,
        bind: Some(BindRule { trigger: "Change", value: BindValue::Text }),
    },
    // Container primitives: material surfaces that host arbitrary children
    // (icons/text/stacks) and take every modifier. `Panel` = the panel-glass
    // surface (Control-Center tiles, window content panels, properties
    // sidebar); `Circle` = a perfectly round container (`size` pins a square
    // box, radius welded to full, content centered) for round buttons,
    // badges and avatar-like elements.
    WidgetSpec {
        name: "Panel",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    WidgetSpec {
        name: "Circle",
        primary_prop: Some("size"),
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    // Container primitive: a REAL fixed-column grid (`columns: n` = n equal
    // 1fr tracks, row-major fill, `.gap()` = column gap, `rowGap:` = row gap
    // in the same spacing scale). This is the engine's `LayoutNode::Grid` —
    // fully implemented in `nexus-layout` since the node types landed, but
    // unreachable from `.nx` until now; every wrap-flow "grid" in the shell
    // was a silent single overflowing row (`.wrap(true)` is a no-op the
    // engine never reads).
    WidgetSpec {
        name: "Grid",
        primary_prop: Some("columns"),
        interactive: false,
        label_prop: None,
        allows_children: true,
        bind: None,
    },
    // Navigation/selection leaves the kit always had but the DSL could not
    // name (settings design handoff). `Select` is the CLOSED trigger only —
    // a glass pill showing the current value plus a chevron; the open option
    // panel is an app-owned `.overlay()`, exactly as the kit crate documents.
    // `Breadcrumbs` renders the whole trail as ONE node, so it shows the path
    // but cannot make individual crumbs tappable — wrap it to navigate.
    WidgetSpec {
        name: "Select",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("placeholder"),
        allows_children: false,
        bind: None,
    },
    WidgetSpec {
        name: "Breadcrumbs",
        primary_prop: Some("items"),
        interactive: false,
        label_prop: None,
        allows_children: false,
        bind: None,
    },
];

#[must_use]
pub fn widget_spec(name: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|spec| spec.name == name)
}

/// The bind rule `widget.prop` synthesizes — `None` when this prop is not the
/// control's primary one, or the control has nothing to write back.
///
/// THE predicate: the symbol collector (which must intern the trigger name) and
/// the lowering (which emits the handler) ask this one question, so they cannot
/// disagree about which props bind. They did: P2 moved the lowering onto the
/// catalog and left the collector on its own list of names, so `SearchBar`'s
/// synthesized handler got symbol 0 — a trigger no interaction resolves to, a
/// handler that could never fire (TASK-0077B P2b).
///
/// The caller still checks that the value is `$state`-bound; that is an AST
/// fact, not a catalog one.
#[must_use]
pub fn bind_rule(widget: &str, prop: &str) -> Option<&'static BindRule> {
    let spec = widget_spec(widget)?;
    (spec.primary_prop == Some(prop)).then_some(())?;
    spec.bind.as_ref()
}
