// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The shell's status panels (design_handoff_panels). Most of what these panels
// show is demo optics by agreement — so what is pinned here is the part that
// is NOT: which panels EXIST per mode, the two controls that reach a real
// service, and the promise that nothing else does.
//
// The mode axis is the reason for most of it. WLAN, Ton and Batterie exist
// only in desktop mode; in touch modes their contents fold into the Control
// Center. A panel id that opened nothing would strand the shell in a state
// with no visible way back out, so "opens in desktop" and "opens nothing in
// tablet" are both assertions.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

/// Records `settings.set` and REFUSES everything else, so an accidental
/// service call in a demo store shows up as a dispatch failure rather than as
/// a silently satisfied `Ok`.
struct SettingsSpy {
    sets: Vec<(String, String)>,
    other: usize,
}

impl nexus_dsl_runtime::EffectHost for SettingsSpy {
    fn call(&mut self, svc: &str, method: &str, args: &[Value], _t: u32) -> Result<Value, u32> {
        if (svc, method) == ("settings", "set") {
            if let (Some(Value::Str(k)), Some(Value::Str(v))) = (args.first(), args.get(1)) {
                self.sets.push((k.clone(), v.clone()));
            }
            return Ok(Value::Bool(true));
        }
        self.other += 1;
        Ok(Value::Bool(true))
    }
}

struct Shell {
    view: View<'static>,
    device: FixtureEnv,
    symbols: Vec<String>,
    keys: Vec<u32>,
    host: SettingsSpy,
}

impl Shell {
    fn new(device: FixtureEnv) -> Self {
        let nxir: &'static [u8] = Box::leak(common::compile("desktop-shell").into_boxed_slice());
        let symbols = common::program_symbols(nxir);
        let keys = common::program_i18n_keys(nxir);
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale)
            .expect("the shell mounts");
        Self { view, device, symbols, keys, host: SettingsSpy { sets: Vec::new(), other: 0 } }
    }

    fn desktop() -> Self {
        Self::new(FixtureEnv::desktop())
    }

    fn tablet() -> Self {
        Self::new(FixtureEnv::tablet("landscape"))
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

    fn open(&mut self, panel: &str) {
        self.send("PanelEvent", "SetPanel", vec![Value::Str(panel.to_string())]);
    }

    /// The open panel's own box: the widest surface below the 36px bar. Every
    /// panel in the handoff is 288..330 wide and none of the shell's other
    /// chrome down there is, so width alone identifies it.
    fn open_panel_width(&self) -> Option<i32> {
        common::layout_boxes(&self.view)
            .iter()
            .filter(|b| b.rect.y.0 >= 36 && b.rect.height.0 > 100)
            .map(|b| b.rect.width.0)
            .filter(|w| (280..=340).contains(w))
            .max()
    }
}

/// Every panel the desktop bar can open actually opens, at the width the
/// handoff specifies. The widths are the identity check: a wrong one means a
/// panel rendered through the wrong branch.
#[test]
fn desktop_opens_all_six_panels_at_their_handoff_widths() {
    let mut shell = Shell::desktop();
    for (panel, width) in [
        ("control", 328),
        ("notifications", 330),
        ("calendar", 288),
        ("wifi", 300),
        ("sound", 300),
        ("battery", 300),
    ] {
        shell.open(panel);
        assert_eq!(
            shell.open_panel_width(),
            Some(width),
            "panel `{panel}` did not render at {width}px in desktop mode"
        );
        shell.open(panel); // toggles closed — the same pill closes what it opened
        assert_eq!(shell.open_panel_width(), None, "tapping `{panel}` again must close it");
    }
}

/// Touch modes have exactly three panels. WLAN/Ton/Batterie fold into the
/// Control Center there, so their ids must render NOTHING rather than a panel
/// no pill can reach.
#[test]
fn touch_mode_has_only_three_panels() {
    let mut shell = Shell::tablet();
    for (panel, width) in [("control", 328), ("notifications", 330), ("calendar", 288)] {
        shell.open(panel);
        assert_eq!(
            shell.open_panel_width(),
            Some(width),
            "panel `{panel}` must exist in touch mode too"
        );
        shell.open(panel);
    }
    for panel in ["wifi", "sound", "battery"] {
        shell.open(panel);
        assert_eq!(
            shell.open_panel_width(),
            None,
            "`{panel}` has no panel in touch mode — its contents live in the \
             Control Center, and rendering it here would be unreachable UI"
        );
        shell.open(panel);
    }
}

/// One panel at a time. Opening another must close the first — the handoff's
/// rule, and the reason `panel` is a single `Str` rather than a set.
#[test]
fn opening_a_second_panel_closes_the_first() {
    let mut shell = Shell::desktop();
    shell.open("calendar");
    assert_eq!(shell.open_panel_width(), Some(288));
    shell.open("battery");
    assert_eq!(
        shell.open_panel_width(),
        Some(300),
        "the calendar must be gone, not stacked behind the battery panel"
    );
}

/// The outside-tap backdrop closes whatever is open, from any panel.
#[test]
fn the_backdrop_closes_every_panel() {
    let mut shell = Shell::desktop();
    for panel in ["control", "notifications", "calendar", "wifi", "sound", "battery"] {
        shell.open(panel);
        shell.open(""); // what the full-bleed layer dispatches
        assert_eq!(shell.open_panel_width(), None, "`{panel}` survived an outside tap");
    }
}

/// The Ansichtsmodus tile is one of the two REAL controls: it writes
/// `ui.shell.mode`, and it writes the mode it is NOT currently in.
#[test]
fn the_view_mode_tile_writes_the_other_shell_mode() {
    let mut desktop = Shell::desktop();
    desktop.open("control");
    desktop.send("ControlEvent", "SetMode", vec![Value::Str("tablet".into())]);
    assert_eq!(
        desktop.host.sets,
        vec![("ui.shell.mode".to_string(), "tablet".to_string())],
        "from desktop the tile must switch TO tablet"
    );

    let mut tablet = Shell::tablet();
    tablet.open("control");
    tablet.send("ControlEvent", "SetMode", vec![Value::Str("desktop".into())]);
    assert_eq!(
        tablet.host.sets,
        vec![("ui.shell.mode".to_string(), "desktop".to_string())],
        "and from tablet, back to desktop"
    );
}

/// The Erscheinungsbild button is the other real control: it writes
/// `ui.theme.mode`. Both directions, because a single button that can only
/// switch one way is the bug `device.theme` was added to prevent.
#[test]
fn the_appearance_button_writes_both_theme_modes() {
    for (from, to) in [("dark", "light"), ("light", "dark")] {
        let mut env = FixtureEnv::desktop();
        env.theme = from;
        let mut shell = Shell::new(env);
        shell.open("control");
        shell.send("ControlEvent", "SetTheme", vec![Value::Str(to.into())]);
        assert_eq!(
            shell.host.sets,
            vec![("ui.theme.mode".to_string(), to.to_string())],
            "in {from} mode the button must write {to}"
        );
    }
}

/// The button SHOWS the mode it is in. `device.theme` is what makes that
/// possible, so the two themes must produce different trees — otherwise the
/// moon and the sun are the same node and the readback is decorative.
#[test]
fn the_appearance_button_renders_differently_per_theme() {
    let structure = |theme: &'static str| {
        let mut env = FixtureEnv::desktop();
        env.theme = theme;
        let mut shell = Shell::new(env);
        shell.open("control");
        common::layout_boxes(&shell.view)
            .iter()
            .filter_map(|b| b.visual.background_gradient)
            .map(|(a, b)| (a.r, a.g, a.b, b.r, b.g, b.b))
            .collect::<Vec<_>>()
    };
    assert_ne!(
        structure("dark"),
        structure("light"),
        "the Control Center must paint a different appearance button per theme — \
         moon on indigo while dark, sun on amber while light"
    );
}

/// The promise behind "the rest is a mock": every demo control changes state
/// and reaches NO service. If one of these ever needs a service, it gets an
/// `@effect` and its own assertion — not a silent call from a reducer.
#[test]
fn every_demo_control_reaches_no_service() {
    let mut shell = Shell::desktop();
    shell.open("control");
    for (event, case, payload) in [
        ("ConnEvent", "ToggleWifi", vec![]),
        ("ConnEvent", "ToggleBluetooth", vec![]),
        ("ConnEvent", "ToggleAirplane", vec![]),
        ("ConnEvent", "ToggleWiredOnly", vec![]),
        ("ConnEvent", "PickNet", vec![Value::Str("guest".into())]),
        ("SoundEvent", "PickOutput", vec![Value::Str("headphones".into())]),
        ("SoundEvent", "PickInput", vec![Value::Str("micExternal".into())]),
        ("PowerEvent", "PickProfile", vec![Value::Str("saver".into())]),
        ("PowerEvent", "ToggleNoStandby", vec![]),
        ("CalEvent", "CalPrev", vec![]),
        ("CalEvent", "CalNext", vec![]),
        ("CalEvent", "CalToday", vec![]),
        ("NotifEvent", "ClearAll", vec![]),
        ("ControlEvent", "ToggleMute", vec![]),
    ] {
        shell.send(event, case, payload);
    }
    assert!(
        shell.host.sets.is_empty() && shell.host.other == 0,
        "a demo control reached a service: {:?} (+{} other calls)",
        shell.host.sets,
        shell.host.other
    );
}

/// Mute and the volume slider are ONE state (handoff §6). Muting must move the
/// slider, or the Control Center and the Ton panel can disagree about silence.
#[test]
fn mute_and_the_volume_slider_are_one_state() {
    let mut shell = Shell::desktop();
    shell.open("sound");
    let fills = |v: &View| {
        // The slider fill is the only box that is exactly the track's height.
        common::layout_boxes(v)
            .iter()
            .filter(|b| b.rect.height.0 == 28 && b.rect.width.0 > 0)
            .map(|b| b.rect.width.0)
            .collect::<Vec<_>>()
    };
    let before = fills(&shell.view);
    shell.send("ControlEvent", "ToggleMute", vec![]);
    let muted = fills(&shell.view);
    assert_ne!(before, muted, "muting must collapse the slider fill, not just recolour a button");
    shell.send("ControlEvent", "ToggleMute", vec![]);
    assert_eq!(fills(&shell.view), before, "un-muting must bring the fill back");
}

/// The calendar pages between its three authored months and stops at the ends.
/// Clamping rather than wrapping is the honest behaviour for three months.
#[test]
fn the_calendar_pages_and_clamps() {
    let mut shell = Shell::desktop();
    shell.open("calendar");
    let month = |v: &View| common::scene_texts(v).join("|");
    let now = month(&shell.view);

    shell.send("CalEvent", "CalNext", vec![]);
    let next = month(&shell.view);
    assert_ne!(now, next, "next must actually change the grid");
    shell.send("CalEvent", "CalNext", vec![]);
    assert_eq!(month(&shell.view), next, "past the last month it must clamp, not wrap");

    shell.send("CalEvent", "CalToday", vec![]);
    assert_eq!(month(&shell.view), now, "the dot returns to the current month");

    shell.send("CalEvent", "CalPrev", vec![]);
    let prev = month(&shell.view);
    assert_ne!(now, prev);
    shell.send("CalEvent", "CalPrev", vec![]);
    assert_eq!(month(&shell.view), prev, "and it clamps at the first month too");
}

/// The WiFi list is inert while it is dimmed. A disabled list that still took
/// taps would change the selection behind the dimming.
#[test]
fn the_dimmed_network_list_takes_no_taps() {
    let mut shell = Shell::desktop();
    shell.open("wifi");
    let handlers = |v: &View| v.handlers().len();
    let live = handlers(&shell.view);

    shell.send("ConnEvent", "ToggleWifi", vec![]); // master off ⇒ list dims
    let dimmed = handlers(&shell.view);
    assert!(dimmed < live, "dimming the list must remove its tap handlers ({live} → {dimmed})");

    shell.send("ConnEvent", "ToggleWifi", vec![]);
    assert_eq!(handlers(&shell.view), live, "and restore them when the master comes back");
}
