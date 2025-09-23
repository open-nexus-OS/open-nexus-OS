//! Client API for nexus-settingsd.
//! Works everywhere by polling the on-disk file; optionally uses a Unix socket when built
//! with `ipc_unix` on Unix platforms.

use crate::{
    model::{Mode, Settings, ThemeMode},
    persist,
};
use crossbeam_channel::{unbounded, Sender};
use parking_lot::Mutex;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[cfg(all(unix, feature = "ipc_unix"))]
mod unix_ipc {
    use super::*;
    use serde_json::json;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixStream,
    };

    pub async fn subscribe_socket(sock_path: &str, tx: Sender<Settings>) -> std::io::Result<()> {
        let stream = UnixStream::connect(sock_path).await?;
        let mut stream = BufReader::new(stream);
        let mut line = String::new();

        // subscribe
        let mut w = stream.get_mut().try_clone()?;
        w.write_all(b"{\"op\":\"subscribe\"}\n").await?;

        loop {
            line.clear();
            let n = stream.read_line(&mut line).await?;
            if n == 0 {
                break;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                if val.get("event").and_then(|e| e.as_str()) == Some("changed") {
                    if let Some(s) = val.get("settings") {
                        if let Ok(st) = serde_json::from_value::<Settings>(s.clone()) {
                            let _ = tx.send(st);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn state_path() -> PathBuf {
    // duplicate of persist's path logic to keep this module independent
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs_next::config_dir())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nexus/state.toml")
}

#[derive(Debug)]
pub struct SettingsClient {
    cache: Arc<Mutex<Settings>>,
    sock_path: Option<String>,
}

impl Default for SettingsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsClient {
    pub fn new() -> Self {
        let s = persist::load();
        std::env::set_var(
            "NEXUS_MODE",
            match s.mode {
                Mode::Mobile => "mobile",
                Mode::Desktop => "desktop",
            },
        );
        std::env::set_var(
            "NEXUS_THEME",
            match s.theme_mode {
                ThemeMode::Dark => "dark",
                ThemeMode::Light => "light",
            },
        );

        let sock = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .map(|d| format!("{}/nexus/settingsd.sock", d));

        Self {
            cache: Arc::new(Mutex::new(s)),
            sock_path: sock,
        }
    }

    pub fn get(&self) -> Settings {
        self.cache.lock().clone()
    }

    pub fn set_mode(&self, mode: Mode) -> std::io::Result<()> {
        let mut s = persist::load();
        s.mode = mode;
        persist::save_atomic(&s)?;
        *self.cache.lock() = s.clone();
        std::env::set_var("NEXUS_MODE", if mode == Mode::Mobile { "mobile" } else { "desktop" });
        Ok(())
    }

    pub fn set_theme_mode(&self, tm: ThemeMode) -> std::io::Result<()> {
        let mut s = persist::load();
        s.theme_mode = tm;
        persist::save_atomic(&s)?;
        *self.cache.lock() = s.clone();
        std::env::set_var("NEXUS_THEME", if tm == ThemeMode::Dark { "dark" } else { "light" });
        Ok(())
    }

    /// Subscribe to changes (socket if available or file polling). Keep the handle alive.
    pub fn subscribe<F>(&self, mut on_change: F) -> SubscribeHandle
    where
        F: FnMut(&Settings) + Send + 'static,
    {
        let (tx, rx) = unbounded::<Settings>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let cache = self.cache.clone();

        // socket first (optional)
        #[cfg(all(unix, feature = "ipc_unix"))]
        let tried_socket = {
            if let Some(path) = &self.sock_path {
                if std::path::Path::new(path).exists() {
                    let p = path.clone();
                    let t = tx.clone();
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let _ = rt.block_on(unix_ipc::subscribe_socket(&p, t));
                    });
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        #[cfg(not(all(unix, feature = "ipc_unix")))]
        let tried_socket = false;

        // fallback: poll file mtime
        if !tried_socket {
            let tx_poll = tx.clone();
            thread::spawn(move || {
                let mut last = std::time::SystemTime::UNIX_EPOCH;
                while !stop2.load(Ordering::Relaxed) {
                    let p = state_path();
                    if let Ok(md) = std::fs::metadata(&p) {
                        if let Ok(mt) = md.modified() {
                            if mt > last {
                                last = mt;
                                let s = persist::load();
                                let _ = tx_poll.send(s);
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            });
        }

        // consumer
        thread::spawn(move || {
            while let Ok(s) = rx.recv() {
                let mut g = cache.lock();
                if *g != s {
                    *g = s.clone();
                    on_change(&s);
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        SubscribeHandle { stop }
    }
}

#[derive(Debug)]
pub struct SubscribeHandle {
    stop: Arc<AtomicBool>,
}
impl Drop for SubscribeHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
