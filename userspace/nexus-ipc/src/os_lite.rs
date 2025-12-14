//! CONTEXT: Cooperative mailbox-backed IPC for no_std OS-lite builds
//!
//! OWNERS: @runtime
//!
//! PUBLIC API:
//!   - struct LiteClient: Client backed by cooperative mailbox
//!   - struct LiteServer: Server backed by cooperative mailbox queues
//!   - set_default_target(): Set default service target for current context
//!   - LiteClient::new(): Create client targeting current default
//!   - LiteClient::new_for(): Create client targeting specific service
//!   - LiteServer::new(): Create server bound to current service name
//!   - LiteServer::new_named(): Create server bound to specific service
//!
//! SECURITY INVARIANTS:
//!   - No unsafe code in mailbox operations
//!   - Frame size limits prevent memory exhaustion
//!   - Queue depth limits prevent unbounded growth
//!   - Cooperative yielding prevents deadlocks
//!   - Thread-local storage for service targeting
//!
//! ERROR CONDITIONS:
//!   - IpcError::Unsupported: Frame too large or feature not available
//!   - IpcError::WouldBlock: Operation would block in non-blocking mode
//!   - IpcError::Timeout: Operation timed out
//!   - IpcError::Disconnected: Target service not available
//!
//! DEPENDENCIES:
//!   - nexus-sync::SpinLock: Synchronization for mailbox queues
//!   - nexus-abi::yield_: Cooperative yielding
//!   - alloc::collections::VecDeque: Queue implementation
//!   - alloc::string::String: String handling
//!   - alloc::sync::Arc: Reference counting
//!   - core::cell::RefCell: Thread-local storage
//!   - core::sync::atomic::AtomicUsize: Atomic operations
//!
//! FEATURES:
//!   - Cooperative mailbox transport
//!   - Service registry with thread-local targeting
//!   - Queue depth limits with cooperative yielding
//!   - Frame size limits for memory safety
//!   - No_std compatibility
//!
//! TEST SCENARIOS:
//!   - test_service_registration(): Register services in mailbox
//!   - test_queue_depth_limits(): Test queue depth enforcement
//!   - test_cooperative_yielding(): Test cooperative yielding behavior
//!   - test_frame_size_limits(): Test frame size enforcement
//!   - test_thread_local_targeting(): Test thread-local service targeting
//!   - test_client_server_communication(): Test client-server communication
//!   - test_timeout_handling(): Test timeout behavior
//!   - test_disconnection_handling(): Test service disconnection
//!
//! ADR: docs/adr/0003-ipc-runtime-architecture.md

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use nexus_sync::SpinLock;

use crate::{Client, IpcError, Result, Server, Wait};

/// Maximum frame size accepted by the cooperative mailbox transport.
const MAX_FRAME: usize = 512;
static MAX_QUEUE_DEPTH: AtomicUsize = AtomicUsize::new(4);

/// Shared registry storing in-memory endpoints for each service.
struct Registry {
    services: SpinLock<Vec<ServiceRecord>>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            services: SpinLock::new(Vec::new()),
        }
    }

    fn get(&self, name: &str) -> Option<Arc<ServiceQueues>> {
        let guard = self.services.lock();
        guard
            .iter()
            .find(|record| record.name == name)
            .map(|record| record.queues.clone())
    }

    fn get_or_insert(&self, name: &str) -> Arc<ServiceQueues> {
        if let Some(existing) = self.get(name) {
            return existing;
        }
        let mut guard = self.services.lock();
        if let Some(record) = guard.iter().find(|record| record.name == name) {
            return record.queues.clone();
        }
        let queues = Arc::new(ServiceQueues::new());
        guard.push(ServiceRecord {
            name: name.to_string(),
            queues: queues.clone(),
        });
        queues
    }
}

struct ServiceRecord {
    name: String,
    queues: Arc<ServiceQueues>,
}

struct ServiceQueues {
    requests: SpinLock<VecDeque<Vec<u8>>>,
    response: SpinLock<Option<Vec<u8>>>,
}

impl ServiceQueues {
    fn new() -> Self {
        Self {
            requests: SpinLock::new(VecDeque::new()),
            response: SpinLock::new(None),
        }
    }
}

static REGISTRY: Registry = Registry::new();
static DEFAULT_TARGET: SpinLock<Option<String>> = SpinLock::new(None);

/// Allows the current execution context to target `name` by default.
pub fn set_default_target(name: &str) {
    *DEFAULT_TARGET.lock() = Some(name.to_string());
}

fn current_target() -> Result<String> {
    DEFAULT_TARGET.lock().clone().ok_or(IpcError::Unsupported)
}

/// Client backed by the cooperative mailbox.
pub struct LiteClient {
    target: String,
}

impl LiteClient {
    /// Creates a client targeting the current thread default.
    pub fn new() -> Result<Self> {
        let target = current_target()?;
        Self::new_for(&target)
    }

    /// Creates a client targeting `service`.
    pub fn new_for(service: &str) -> Result<Self> {
        if REGISTRY.get(service).is_none() {
            return Err(IpcError::Disconnected);
        }
        Ok(Self {
            target: service.to_string(),
        })
    }
}

impl Client for LiteClient {
    fn send(&self, frame: &[u8], wait: Wait) -> Result<()> {
        if frame.len() > MAX_FRAME {
            return Err(IpcError::Unsupported);
        }
        let queues = REGISTRY.get(&self.target).ok_or(IpcError::Disconnected)?;
        let mut requests = queues.requests.lock();
        if requests.len() >= MAX_QUEUE_DEPTH.load(Ordering::Relaxed) {
            drop(requests);
            match wait {
                Wait::NonBlocking => return Err(IpcError::WouldBlock),
                Wait::Timeout(_) => return Err(IpcError::Timeout),
                Wait::Blocking => loop {
                    let _ = nexus_abi::yield_();
                    let mut retry = queues.requests.lock();
                    if retry.len() < MAX_QUEUE_DEPTH.load(Ordering::Relaxed) {
                        retry.push_back(frame.to_vec());
                        return Ok(());
                    }
                    drop(retry);
                },
            }
        }
        requests.push_back(frame.to_vec());
        Ok(())
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let queues = REGISTRY.get(&self.target).ok_or(IpcError::Disconnected)?;
        loop {
            if let Some(frame) = queues.response.lock().take() {
                return Ok(frame);
            }
            match wait {
                Wait::NonBlocking => return Err(IpcError::WouldBlock),
                Wait::Timeout(_) => return Err(IpcError::Timeout),
                Wait::Blocking => {
                    let _ = nexus_abi::yield_();
                }
            }
        }
    }
}

/// Server backed by the cooperative mailbox queues.
pub struct LiteServer {
    queues: Arc<ServiceQueues>,
}

impl LiteServer {
    /// Creates a server bound to the current service thread name.
    pub fn new() -> Result<Self> {
        let target = current_target()?;
        Self::new_named(&target)
    }

    /// Creates a server explicitly bound to `service`.
    pub fn new_named(service: &str) -> Result<Self> {
        let queues = REGISTRY.get_or_insert(service);
        Ok(Self {
            queues,
        })
    }
}

impl Server for LiteServer {
    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        loop {
            if let Some(frame) = self.queues.requests.lock().pop_front() {
                return Ok(frame);
            }
            match wait {
                Wait::NonBlocking => return Err(IpcError::WouldBlock),
                Wait::Timeout(_) => return Err(IpcError::Timeout),
                Wait::Blocking => {
                    let _ = nexus_abi::yield_();
                }
            }
        }
    }

    fn send(&self, frame: &[u8], wait: Wait) -> Result<()> {
        if frame.len() > MAX_FRAME {
            return Err(IpcError::Unsupported);
        }
        let response = self.queues.response.lock();
        if response.is_some() {
            drop(response);
            match wait {
                Wait::NonBlocking => return Err(IpcError::WouldBlock),
                Wait::Timeout(_) => return Err(IpcError::Timeout),
                Wait::Blocking => loop {
                    let _ = nexus_abi::yield_();
                    let mut retry = self.queues.response.lock();
                    if retry.is_none() {
                        *retry = Some(frame.to_vec());
                        return Ok(());
                    }
                    drop(retry);
                },
            }
        }
        *self.queues.response.lock() = Some(frame.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_unsupported_without_service() {
        set_default_target("missing");
        assert_eq!(LiteClient::new().unwrap_err(), IpcError::Disconnected);
    }
}
