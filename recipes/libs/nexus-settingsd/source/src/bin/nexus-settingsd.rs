//! Binary entrypoint: run the daemon forever.

use nexus_settingsd::daemon::SettingsDaemon;

fn main() {
    env_logger::init();
    let d = SettingsDaemon::new();
    d.run();
}
