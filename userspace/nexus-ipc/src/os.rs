// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel-backed IPC implementation.
//!
//! Temporary in-process router for OS builds: until the kernel syscall surface
//! is available, services and clients communicate over per-service loopback
//! channels keyed by the spawning thread name (`svc-<service>`). Clients set a
//! default target via [`set_default_target`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};

use crate::{Client, IpcError, Result, Server, Wait};

// Global router holding per-service channels.
struct ServiceChannels {
    request_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
    response_tx: Sender<Vec<u8>>,
}

struct ClientChannels {
    request_tx: Sender<Vec<u8>>,
    response_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
}

struct Router {
    services: HashMap<String, (ServiceChannels, ClientChannels)>,
}

impl Router {
    fn get_or_create(&mut self, name: &str) -> (&ServiceChannels, &ClientChannels) {
        self.services.entry(name.to_string()).or_insert_with(|| {
            let (req_tx, req_rx) = mpsc::channel::<Vec<u8>>();
            let (rsp_tx, rsp_rx) = mpsc::channel::<Vec<u8>>();
            (
                ServiceChannels { request_rx: Arc::new(Mutex::new(req_rx)), response_tx: rsp_tx },
                ClientChannels { request_tx: req_tx, response_rx: Arc::new(Mutex::new(rsp_rx)) },
            )
        });
        // SAFETY: just inserted or existed
        let (svc, cli) = self.services.get(name).unwrap();
        (svc, cli)
    }
}

fn router() -> &'static Mutex<Router> {
    static ROUTER: OnceLock<Mutex<Router>> = OnceLock::new();
    ROUTER.get_or_init(|| Mutex::new(Router { services: HashMap::new() }))
}

// Thread-local default target for clients.
thread_local! {
    static DEFAULT_TARGET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Sets the default service target for the current thread's clients.
pub fn set_default_target(name: &str) {
    DEFAULT_TARGET.with(|slot| slot.replace(Some(name.to_string())));
}

fn current_service_name() -> Option<String> {
    std::thread::current().name().map(|n| n.to_string()).and_then(|n| {
        if let Some(rest) = n.strip_prefix("svc-") {
            Some(rest.to_string())
        } else {
            Some(n)
        }
    })
}

/// Client backed by kernel IPC. The implementation is provided once the kernel
/// syscall layer is ready; for now all operations return [`IpcError::Unsupported`].
pub struct KernelClient {
    request_tx: Sender<Vec<u8>>,
    response_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
}

impl KernelClient {
    /// Creates a new client bound to the thread's default target service set
    /// via [`set_default_target`].
    pub fn new() -> Result<Self> {
        let target = DEFAULT_TARGET
            .with(|slot| slot.borrow().clone())
            .ok_or(IpcError::Unsupported)?;
        let guard = router().lock().unwrap();
        let (_svc, cli) = guard.services.get(&target).ok_or(IpcError::Disconnected)?;
        Ok(Self { request_tx: cli.request_tx.clone(), response_rx: cli.response_rx.clone() })
    }

    /// Creates a client for a specific target service name.
    pub fn new_for(target: &str) -> Result<Self> {
        let guard = router().lock().unwrap();
        let (_svc, cli) = guard.services.get(target).ok_or(IpcError::Disconnected)?;
        Ok(Self { request_tx: cli.request_tx.clone(), response_rx: cli.response_rx.clone() })
    }
}

impl Client for KernelClient {
    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.request_tx.send(frame.to_vec()).map_err(|_| IpcError::Disconnected)
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self.response_rx.lock().unwrap();
        match wait {
            Wait::Blocking => receiver.recv().map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver.try_recv().map_err(|err| match err {
                TryRecvError::Empty => IpcError::WouldBlock,
                TryRecvError::Disconnected => IpcError::Disconnected,
            }),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver.try_recv().map_err(|err| match err {
                        TryRecvError::Empty => IpcError::WouldBlock,
                        TryRecvError::Disconnected => IpcError::Disconnected,
                    });
                }
                receiver.recv_timeout(timeout).map_err(|err| match err {
                    RecvTimeoutError::Timeout => IpcError::Timeout,
                    RecvTimeoutError::Disconnected => IpcError::Disconnected,
                })
            }
        }
    }
}

/// Server backed by kernel IPC.
pub struct KernelServer {
    request_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
    response_tx: Sender<Vec<u8>>,
}

impl KernelServer {
    /// Creates a server bound to the current thread's service name. The thread
    /// should be spawned by init as `svc-<service>`.
    pub fn new() -> Result<Self> {
        let name = current_service_name().ok_or(IpcError::Unsupported)?;
        let mut guard = router().lock().unwrap();
        let (svc, cli) = guard.get_or_create(&name);
        let _ = cli; // silence unused in some builds
        Ok(Self { request_rx: svc.request_rx.clone(), response_tx: svc.response_tx.clone() })
    }
}

impl Server for KernelServer {
    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self.request_rx.lock().unwrap();
        match wait {
            Wait::Blocking => receiver.recv().map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver.try_recv().map_err(|err| match err {
                TryRecvError::Empty => IpcError::WouldBlock,
                TryRecvError::Disconnected => IpcError::Disconnected,
            }),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver.try_recv().map_err(|err| match err {
                        TryRecvError::Empty => IpcError::WouldBlock,
                        TryRecvError::Disconnected => IpcError::Disconnected,
                    });
                }
                receiver.recv_timeout(timeout).map_err(|err| match err {
                    RecvTimeoutError::Timeout => IpcError::Timeout,
                    RecvTimeoutError::Disconnected => IpcError::Disconnected,
                })
            }
        }
    }

    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.response_tx.send(frame.to_vec()).map_err(|_| IpcError::Disconnected)
    }
}
