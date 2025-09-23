//! Daemon: owns the in-memory Settings, persists, and (optionally) serves a Unix socket.

use crate::{
    model::{Mode, Settings, ThemeMode},
    persist,
};
use parking_lot::RwLock;
use std::{sync::Arc, thread, time::Duration};

#[derive(Clone)]
pub struct SettingsDaemon {
    state: Arc<RwLock<Settings>>,
}

impl SettingsDaemon {
    pub fn new() -> Self {
        let s = persist::load();
        std::env::set_var("NEXUS_MODE", if s.mode == Mode::Mobile { "mobile" } else { "desktop" });
        std::env::set_var(
            "NEXUS_THEME",
            if s.theme_mode == ThemeMode::Dark {
                "dark"
            } else {
                "light"
            },
        );
        Self {
            state: Arc::new(RwLock::new(s)),
        }
    }

    pub fn get(&self) -> Settings {
        self.state.read().clone()
    }

    pub fn set(&self, s: Settings) {
        *self.state.write() = s.clone();
        let _ = persist::save_atomic(&s);
        std::env::set_var("NEXUS_MODE", if s.mode == Mode::Mobile { "mobile" } else { "desktop" });
        std::env::set_var(
            "NEXUS_THEME",
            if s.theme_mode == ThemeMode::Dark {
                "dark"
            } else {
                "light"
            },
        );
    }

    pub fn set_mode(&self, m: Mode) {
        let mut s = self.state.write();
        s.mode = m;
        let s2 = s.clone();
        drop(s);
        self.set(s2);
    }

    pub fn set_theme_mode(&self, tm: ThemeMode) {
        let mut s = self.state.write();
        s.theme_mode = tm;
        let s2 = s.clone();
        drop(s);
        self.set(s2);
    }

    pub fn run(self) {
        #[cfg(all(unix, feature = "ipc_unix"))]
        self.spawn_unix_socket();
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }
}

#[cfg(all(unix, feature = "ipc_unix"))]
impl SettingsDaemon {
    use log::{info, warn};
    use serde_json::json;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };
    use std::os::unix::fs::PermissionsExt;

    fn socket_path() -> std::path::PathBuf {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let dir = std::path::Path::new(&base).join("nexus");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("settingsd.sock")
    }

    fn spawn_unix_socket(&self) {
        let state = self.state.clone();
        let path = Self::socket_path();
        let _ = std::fs::remove_file(&path);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let listener = match UnixListener::bind(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        warn!("settingsd: cannot bind socket: {e}");
                        return;
                    }
                };
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                info!("nexus-settingsd: listening at {:?}", path);

                loop {
                    let (mut stream, _) = match listener.accept().await {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("accept error: {e}");
                            continue;
                        }
                    };

                    let st = state.clone();
                    tokio::spawn(async move {
                        let (r, mut w) = stream.split();
                        let mut r = BufReader::new(r);
                        let mut line = String::new();

                        loop {
                            line.clear();
                            let n = match r.read_line(&mut line).await {
                                Ok(n) => n,
                                Err(_) => break,
                            };
                            if n == 0 {
                                break;
                            }
                            let req = serde_json::from_str::<serde_json::Value>(&line).unwrap_or_default();
                            match req.get("op").and_then(|v| v.as_str()) {
                                Some("get") => {
                                    let s = st.read().clone();
                                    let _ = w
                                        .write_all(
                                            format!("{}\n", json!({"ok":true,"settings":s})).as_bytes(),
                                        )
                                        .await;
                                }
                                Some("set_field") => {
                                    let mut s = st.write();
                                    if let (Some(field), Some(value)) =
                                        (req.get("field").and_then(|v| v.as_str()), req.get("value"))
                                    {
                                        match field {
                                            "mode" => {
                                                if let Some(v) = value.as_str() {
                                                    s.mode = if v.eq_ignore_ascii_case("mobile") {
                                                        Mode::Mobile
                                                    } else {
                                                        Mode::Desktop
                                                    }
                                                }
                                            }
                                            "theme_mode" => {
                                                if let Some(v) = value.as_str() {
                                                    s.theme_mode = if v.eq_ignore_ascii_case("dark") {
                                                        ThemeMode::Dark
                                                    } else {
                                                        ThemeMode::Light
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    let s2 = s.clone();
                                    drop(s);
                                    let _ = crate::persist::save_atomic(&s2);
                                    let _ = w.write_all(b"{\"ok\":true}\n").await;
                                    let _ = w
                                        .write_all(
                                            format!("{}\n", json!({"event":"changed","settings":s2})).as_bytes(),
                                        )
                                        .await;
                                }
                                Some("subscribe") => {
                                    let s = st.read().clone();
                                    let _ = w
                                        .write_all(
                                            format!("{}\n", json!({"event":"changed","settings":s})).as_bytes(),
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = w.write_all(b"{\"ok\":false,\"err\":\"unknown_op\"}\n").await;
                                }
                            }
                        }
                    });
                }
            });
        });
    }
}
