// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Settings conformance: the design handoff's information architecture is
// mostly demo optics on purpose, so what these tests pin is the part that is
// NOT — the navigation state machine, and the two areas that reach a real
// service (`ui.theme.*` and the region keys).

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct SettingsSpy {
    sets: Vec<(String, String)>,
    /// `(method, arg)` per `svc.updates.*` call (TASK-0140).
    updates_calls: Vec<(String, String)>,
    /// Outcome the mutating updates verbs answer with — `Err(4)` is the
    /// policy deny (`ERR_SVC_DENIED`), the state the page must render.
    updates_action: Result<bool, u32>,
    /// Program symbols — the status record is symbol-id keyed, mirroring
    /// the REAL row shape app-host's effect host builds (the hover_sweep
    /// rule: a simpler spy record green-lights a broken boot).
    symbols: Vec<String>,
}

impl SettingsSpy {
    fn sym(&self, name: &str) -> Option<u32> {
        self.symbols.iter().position(|s| s == name).map(|i| i as u32)
    }

    /// The REAL UpdateStat shape from app-host `effect_updates.rs`: all-Str
    /// fields, only interned symbols, sorted by symbol id.
    fn update_stat(&self) -> Value {
        let mut fields: Vec<(u32, Value)> = Vec::new();
        for (name, val) in [
            ("slot", "a"),
            ("pending", "-"),
            ("tries", "0"),
            ("committed", "yes"),
            ("synced", "yes"),
            ("floor", "1"),
            ("staged", ""),
        ] {
            if let Some(sym) = self.sym(name) {
                fields.push((sym, Value::Str(val.into())));
            }
        }
        fields.sort_by_key(|(s, _)| *s);
        Value::Record(fields)
    }
}

impl nexus_dsl_runtime::EffectHost for SettingsSpy {
    fn call(
        &mut self,
        svc: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (svc, method) {
            ("settings", "set") => {
                if let (Some(Value::Str(k)), Some(Value::Str(v))) = (args.first(), args.get(1)) {
                    self.sets.push((k.clone(), v.clone()));
                }
                Ok(Value::Bool(true))
            }
            ("updates", "status") => {
                self.updates_calls.push(("status".into(), String::new()));
                let stat = self.update_stat();
                Ok(Value::List(vec![stat]))
            }
            ("updates", "feed") => {
                self.updates_calls.push(("feed".into(), String::new()));
                Ok(Value::List(vec![Value::Str("os-B.nxs".into())]))
            }
            ("updates", "check") => {
                self.updates_calls.push(("check".into(), String::new()));
                Ok(Value::Int(1))
            }
            ("updates", "stage") => {
                let name = match args.first() {
                    Some(Value::Str(n)) => n.clone(),
                    _ => String::new(),
                };
                self.updates_calls.push(("stage".into(), name));
                self.updates_action.map(Value::Bool)
            }
            ("updates", "switch") => {
                self.updates_calls.push(("switch".into(), String::new()));
                self.updates_action.map(Value::Bool)
            }
            ("updates", "rollback") => {
                self.updates_calls.push(("rollback".into(), String::new()));
                self.updates_action.map(Value::Bool)
            }
            _ => Err(0),
        }
    }
}

struct Harness {
    view: View<'static>,
    device: FixtureEnv,
    symbols: Vec<String>,
    keys: Vec<u32>,
    host: SettingsSpy,
}

impl Harness {
    fn new() -> Self {
        // `View` borrows the program bytes; a test that returns the view has
        // to hand it something that outlives the call. Leaking one compile per
        // test case is cheaper than threading a lifetime through every helper.
        let nxir: &'static [u8] = Box::leak(common::compile("settings").into_boxed_slice());
        let device = FixtureEnv::tablet("landscape");
        let symbols = common::program_symbols(nxir);
        let keys = common::program_i18n_keys(nxir);
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale)
            .expect("settings mounts");
        let host = SettingsSpy {
            sets: Vec::new(),
            updates_calls: Vec::new(),
            updates_action: Ok(true),
            symbols: symbols.clone(),
        };
        Self { view, device, symbols, keys, host }
    }

    fn send(&mut self, event: &str, case: &str, payload: Vec<Value>) {
        common::dispatch_with_keys(
            &mut self.view,
            &self.device,
            &mut self.host,
            &self.symbols,
            &self.keys,
            event,
            case,
            payload,
        );
    }

    fn texts(&self) -> Vec<String> {
        common::scene_texts(&self.view)
    }

    fn shows(&self, needle: &str) -> bool {
        self.texts().iter().any(|t| t == needle)
    }

    /// Taps every handler-bearing box inside the content pane's BODY until the
    /// spy records `key`.
    ///
    /// The bounds are not decoration. Left of 260 is the nav rail and above
    /// 110 is the top chrome plus the breadcrumb — and the breadcrumb
    /// dispatches `Back`, so a sweep that included it would navigate off the
    /// page after one tap and then keep tapping a stale box list.
    fn tap_content_until(&mut self, key: &str) -> bool {
        let boxes = common::layout_boxes(&self.view);
        let ids: Vec<usize> = self.view.handlers().iter().map(|(id, _)| *id).collect();
        for id in ids {
            let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
            if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
                continue;
            }
            if b.rect.y.as_i32() < 110 || b.rect.x.as_i32() < 260 {
                continue;
            }
            let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
            let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
            let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
            let _ = self.view.pointer(
                &nexus_theme_tokens::BaseTokens,
                &self.device,
                &locale,
                &mut self.host,
                &boxes,
                "Tap",
                cx,
                cy,
            );
            if self.host.sets.iter().any(|(k, _)| k == key) {
                return true;
            }
        }
        false
    }
}

/// The app lands on the FIRST SECTION in browse mode — the handoff's initial
/// state (`nav: 'connections', mode: 'browse'`), not on the overview.
#[test]
fn settings_opens_on_the_connections_section() {
    let h = Harness::new();
    assert!(
        h.shows("MyHomeIsWhereMyWifiIs"),
        "landing must show the Connections rows: {:?}",
        h.texts()
    );
    assert!(
        !h.shows("Wi-Fi, internet, Bluetooth, VPN"),
        "the overview card grid must NOT be the landing mode"
    );
}

/// The overview is a MODE the user enters (app chip / sidebar header / ⋯
/// menu → GoOverview): all twelve cards with their descriptions — the one
/// place the full section list is proven complete. The card DESCRIPTIONS are
/// asserted (not the titles, which the sidebar also shows).
#[test]
fn go_overview_shows_all_twelve_cards() {
    let mut h = Harness::new();
    h.send("NavEvent", "GoOverview", vec![]);
    let texts = h.texts();
    for desc in [
        "Wi-Fi, internet, Bluetooth, VPN",
        "Input devices, printers",
        "Output, input, tone mode",
        "Resolution, night mode",
        "Status bar, do not disturb",
        "Wallpaper, appearance",
        "Default apps",
        "Time, language, region",
        "Manage",
        "Biometric data",
        "Battery protection, usage",
        "Status, hardware",
    ] {
        assert!(texts.iter().any(|t| t == desc), "overview misses {desc}: {texts:?}");
    }
}

/// Every sidebar id reaches its section. The ids ARE the section values, so
/// this is also the guard that `SectionView`'s chain and `NavSidebar`'s list
/// never drift apart: a typo in either shows the fall-through arm (Geräte
/// Info) instead of the section asked for.
#[test]
fn every_section_id_renders_its_own_section() {
    let cases = [
        ("connections", "MyHomeIsWhereMyWifiIs"),
        ("devices", "Pairing mode"),
        ("sound", "Tone mode & schedule"),
        ("display", "Refresh rate"),
        ("notif", "Badges on app icons"),
        ("personal", "Reduce transparency"),
        ("apps", "Ask before installing"),
        ("general", "Set automatically"),
        ("accounts", "Other accounts"),
        ("privacy", "Permission log"),
        ("power", "Usage per app"),
        ("info", "System update"),
    ];
    for (id, marker) in cases {
        let mut h = Harness::new();
        h.send("NavEvent", "WinMenuPick", vec![Value::Str(id.into())]);
        assert!(h.shows(marker), "section {id} does not show {marker}: {:?}", h.texts());
    }
}

/// Back walks the handoff's chain: sub-page → section → overview. The
/// appearance page is the one sub-page that exists, so it is the only path
/// that exercises all three steps.
#[test]
fn back_walks_subpage_then_section_then_overview() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("personal".into())]);
    h.send("NavEvent", "OpenSub", vec![Value::Str("appearance".into())]);
    assert!(h.shows("Accent colour"), "appearance sub-page did not open: {:?}", h.texts());

    h.send("NavEvent", "Back", vec![]);
    assert!(h.shows("Reduce transparency"), "back did not land on the section");
    assert!(!h.shows("Accent colour"), "the sub-page is still showing");

    h.send("NavEvent", "Back", vec![]);
    assert!(h.shows("Biometric data"), "back did not land on the overview: {:?}", h.texts());
}

/// The appearance page's mode tiles reach the REAL theme chain — the handoff's
/// Hell/Dunkel write `ui.theme.mode` through `svc.settings`.
#[test]
fn appearance_mode_tiles_reach_settings_set() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("personal".into())]);
    h.send("NavEvent", "OpenSub", vec![Value::Str("appearance".into())]);
    assert!(h.tap_content_until("ui.theme.mode"), "no theme set reached the host");
}

/// All NINE handoff accents write, not just the six the palette had before
/// this app existed. A swatch whose name settingsd refuses would tap dead, so
/// the set of names is asserted here and mirrored by settingsd's own
/// `theme_accent_validator_pins_the_curated_palette`.
#[test]
fn all_nine_accents_reach_settings_set() {
    for accent in
        ["default", "teal", "green", "amber", "orange", "red", "pink", "violet", "graphite"]
    {
        let mut h = Harness::new();
        h.send("AppearanceEvent", "SetAccent", vec![Value::Str(accent.into())]);
        assert!(
            h.host.sets.iter().any(|(k, v)| k == "ui.theme.accent" && v == accent),
            "accent {accent} never reached the host: {:?}",
            h.host.sets
        );
    }
}

/// "Automatisch" is drawn but writes NOTHING — windowd has no auto mode. The
/// tile has to move the local selection (so it can show as picked) without
/// touching the theme chain; anything else would be a control that lies.
#[test]
fn auto_mode_selects_locally_and_writes_nothing() {
    let mut h = Harness::new();
    h.send("AppearanceEvent", "SetModeAuto", vec![]);
    assert!(h.host.sets.is_empty(), "the auto tile must not write a theme mode: {:?}", h.host.sets);
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("personal".into())]);
    assert!(h.shows("Automatic"), "the section row does not reflect the auto pick");
}

/// The region pickers are the second real area: language, time zone and
/// keyboard layout each write the settingsd key their service consumes.
#[test]
fn region_picks_reach_their_settings_keys() {
    let cases = [
        ("SetLocale", "en-US", "ui.locale"),
        ("SetZone", "Asia/Tokyo", "time.zone"),
        ("SetKeymap", "us", "input.keymap"),
        ("SetCountry", "US", "region.country"),
        ("SetHourFmt", "12h", "time.format"),
    ];
    for (case, value, key) in cases {
        let mut h = Harness::new();
        h.send("RegionEvent", case, vec![Value::Str(value.into())]);
        assert!(
            h.host.sets.iter().any(|(k, v)| k == key && v == value),
            "{case} did not write {key}: {:?}",
            h.host.sets
        );
    }
}

/// Opening a picker sheet closes on pick: the sheet is an `.overlay()` that
/// swallows every tap while it is up, so a pick that left it open would trap
/// the window.
#[test]
fn picking_a_value_closes_the_picker_sheet() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("general".into())]);
    h.send("RegionEvent", "OpenPicker", vec![Value::Str("zone".into())]);
    assert!(h.shows("Los Angeles"), "the zone sheet did not open: {:?}", h.texts());

    h.send("RegionEvent", "SetZone", vec![Value::Str("Asia/Tokyo".into())]);
    assert!(!h.shows("Los Angeles"), "the sheet is still open after a pick");
}

/// A demo switch is exactly that: it flips on screen and reaches no service.
/// This is the line between the two halves of the app, so it is asserted
/// rather than assumed.
#[test]
fn demo_switches_reach_no_service() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("privacy".into())]);
    let _ = h.tap_content_until("nothing.will.match");
    assert!(h.host.sets.is_empty(), "a demo section wrote a setting: {:?}", h.host.sets);
}

/// Like `tap_content_until`, but the goal is a rendered text instead of a
/// recorded write — used to walk INTO the updates sub-page via its real row.
impl Harness {
    fn tap_content_until_shows(&mut self, needle: &str) -> bool {
        let boxes = common::layout_boxes(&self.view);
        let ids: Vec<usize> = self.view.handlers().iter().map(|(id, _)| *id).collect();
        for id in ids {
            let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
            if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
                continue;
            }
            if b.rect.y.as_i32() < 110 || b.rect.x.as_i32() < 260 {
                continue;
            }
            let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
            let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
            let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
            let _ = self.view.pointer(
                &nexus_theme_tokens::BaseTokens,
                &self.device,
                &locale,
                &mut self.host,
                &boxes,
                "Tap",
                cx,
                cy,
            );
            if self.shows(needle) {
                return true;
            }
        }
        false
    }
}

/// TASK-0140: the Info section's "System update" row pushes the updates
/// sub-page, and entering it loads SERVICE truth (status → feed → check
/// chain) — the fields on screen are the spy's record values, not app-made.
#[test]
fn info_update_row_opens_updates_and_shows_service_truth() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("info".into())]);
    assert!(
        h.tap_content_until_shows("Update status"),
        "the System update row did not open the updates sub-page: {:?}",
        h.texts()
    );
    assert!(h.shows("Active slot"), "status rows missing: {:?}", h.texts());
    assert!(h.shows("a"), "the active-slot VALUE must come from the service record");
    assert!(h.shows("os-B.nxs"), "the feed candidate must be listed: {:?}", h.texts());
    let methods: Vec<&str> = h.host.updates_calls.iter().map(|(m, _)| m.as_str()).collect();
    assert!(
        methods.contains(&"status") && methods.contains(&"feed") && methods.contains(&"check"),
        "entering the page must run the status→feed→check chain: {methods:?}"
    );
}

/// A policy deny (`updates.manage`, ERR_SVC_DENIED = 4) renders as its own
/// honest state — never a fake success, never a generic failure.
#[test]
fn updates_action_denied_renders_the_denied_state() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("info".into())]);
    h.send("NavEvent", "OpenSub", vec![Value::Str("updates".into())]);
    h.host.updates_action = Err(4);
    h.send("UpdatesEvent", "UpdSwitch", vec![]);
    assert!(h.shows("Not permitted"), "deny must render its own state: {:?}", h.texts());

    // A transport failure is NOT the denied state.
    h.host.updates_action = Err(1);
    h.send("UpdatesEvent", "UpdRollback", vec![]);
    assert!(h.shows("Action failed"), "failure must render as failure: {:?}", h.texts());
}

/// Tapping a feed candidate asks the ENGINE to stage it (`svc.updates.stage`
/// with the candidate name) and a completed action re-reads the status.
#[test]
fn updates_stage_tap_reaches_the_engine() {
    let mut h = Harness::new();
    h.send("NavEvent", "WinMenuPick", vec![Value::Str("info".into())]);
    h.send("NavEvent", "OpenSub", vec![Value::Str("updates".into())]);
    let before = h.host.updates_calls.iter().filter(|(m, _)| m == "status").count();
    let _ = h.tap_content_until("nothing.will.match");
    assert!(
        h.host.updates_calls.iter().any(|(m, a)| m == "stage" && a == "os-B.nxs"),
        "no stage call with the candidate name: {:?}",
        h.host.updates_calls
    );
    let after = h.host.updates_calls.iter().filter(|(m, _)| m == "status").count();
    assert!(after > before, "a completed action must re-read the status");
}
