//! CONTEXT: Notification daemon entrypoint wiring to service logic
//! INTENT: Notifications, channels, badges, actions
//! IDL (target): post(channel,payload), cancel(id), subscribe()
//! DEPS: systemui (display)
//! READINESS: print "notifd: ready"; register/heartbeat with samgr
//! TESTS: post/cancel roundtrip; subscribe emits
//! Notification daemon entry point.

fn main() {
    notif::run();
    println!("notifd: ready");
}
