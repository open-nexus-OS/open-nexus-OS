//! CONTEXT: System UI service entrypoint wiring to service logic
//! INTENT: Statusbar, launcher, shell UI
//! IDL (target): showLauncher(), setStatus(icon,state), setTheme(theme)
//! DEPS: compositor/ime/notifd
//! READINESS: print "systemui: ready"; register/heartbeat with samgr
//! TESTS: show launcher mock
fn main() {
    systemui::run();
}
