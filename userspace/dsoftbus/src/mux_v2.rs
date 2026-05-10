// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Mux v2 contract core (typed domains + deterministic bounded rejects)
//! OWNERS: @runtime
//! STATUS: In Progress (host contract and integration surfaces)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs, userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs, userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

pub type Tick = u64;

pub const MAX_STREAM_COUNT: usize = 128;
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 32 * 1024;
pub const MAX_BUFFERED_BYTES_PER_STREAM: usize = 256 * 1024;
pub const DEFAULT_INITIAL_STREAM_CREDIT: u32 = 64 * 1024;
pub const HIGH_PRIORITY_BURST_LIMIT: u8 = 8;
pub const KEEPALIVE_INTERVAL_TICKS: Tick = 3;
pub const KEEPALIVE_TIMEOUT_TICKS: Tick = 9;

pub const REJECT_FRAME_OVERSIZE: &str = "mux.reject.frame_oversize";
pub const REJECT_INVALID_STREAM_STATE_TRANSITION: &str =
    "mux.reject.invalid_stream_state_transition";
pub const REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW: &str =
    "mux.reject.window_credit_overflow_or_underflow";
pub const REJECT_UNKNOWN_STREAM_FRAME: &str = "mux.reject.unknown_stream_frame";
pub const REJECT_UNAUTHENTICATED_SESSION: &str = "mux.reject.unauthenticated_session";
pub const REJECT_STREAM_LIMIT_EXCEEDED: &str = "mux.reject.stream_limit_exceeded";
pub const REJECT_DUPLICATE_STREAM_NAME: &str = "mux.reject.duplicate_stream_name";
pub const REJECT_INVALID_STREAM_NAME: &str = "mux.reject.invalid_stream_name";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StreamId(u32);

impl StreamId {
    pub fn new(raw: u32) -> Option<Self> {
        if raw == 0 {
            None
        } else {
            Some(Self(raw))
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PriorityClass(u8);

impl PriorityClass {
    pub const HIGHEST: u8 = 0;
    pub const LOWEST: u8 = 7;

    pub fn new(raw: u8) -> Option<Self> {
        if raw <= Self::LOWEST {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct StreamName(String);

impl StreamName {
    pub const MAX_LEN: usize = 64;

    pub fn new(raw: impl Into<String>) -> Result<Self, MuxReject> {
        let value = raw.into();
        if value.is_empty() || value.len() > Self::MAX_LEN {
            return Err(MuxReject::new(REJECT_INVALID_STREAM_NAME));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowCredit(u32);

impl WindowCredit {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MuxReject {
    label: &'static str,
}

impl MuxReject {
    const fn new(label: &'static str) -> Self {
        Self { label }
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub const fn label(self) -> &'static str {
        self.label
    }
}

impl fmt::Display for MuxReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamState {
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamTransition {
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    SendClose,
    ReceiveClose,
    Reset,
}

#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionOutcome {
    pub next_state: StreamState,
}

#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendBudgetOutcome {
    Sent { remaining_credit: WindowCredit },
    WouldBlock { remaining_credit: WindowCredit },
}

#[must_use]
#[cfg_attr(nexus_env = "os", allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeepaliveVerdict {
    Healthy,
    SendPing,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboundFrame {
    Open,
    OpenAck,
    Data { payload_len: usize },
    WindowUpdate { delta: i64 },
    Rst,
    Ping,
    Pong,
    Close,
}

#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameApplyOutcome {
    StreamOpened {
        state: StreamState,
    },
    OpenAcked {
        state: StreamState,
    },
    DataAccepted {
        buffered_bytes: usize,
        remaining_credit: WindowCredit,
    },
    WindowUpdated {
        credit: WindowCredit,
    },
    StreamTransitioned {
        state: StreamState,
    },
    KeepaliveObserved,
    PingResponseRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MuxWireEvent {
    Open {
        stream_id: StreamId,
        priority: PriorityClass,
        name: StreamName,
    },
    OpenAck {
        stream_id: StreamId,
        priority: PriorityClass,
    },
    Data {
        stream_id: StreamId,
        priority: PriorityClass,
        payload_len: usize,
    },
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    WindowUpdate {
        stream_id: StreamId,
        priority: PriorityClass,
        delta: i64,
    },
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    Rst {
        stream_id: StreamId,
        priority: PriorityClass,
    },
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    Close {
        stream_id: StreamId,
        priority: PriorityClass,
    },
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    Ping {
        stream_id: StreamId,
        priority: PriorityClass,
    },
    Pong {
        stream_id: StreamId,
        priority: PriorityClass,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedStream {
    pub stream_id: StreamId,
    pub priority: PriorityClass,
    pub name: StreamName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeepalivePolicy {
    pub interval_ticks: Tick,
    pub timeout_ticks: Tick,
}

impl Default for KeepalivePolicy {
    fn default() -> Self {
        Self {
            interval_ticks: KEEPALIVE_INTERVAL_TICKS,
            timeout_ticks: KEEPALIVE_TIMEOUT_TICKS,
        }
    }
}

#[derive(Debug)]
struct StreamContext {
    state: StreamState,
    open_acked: bool,
    priority: PriorityClass,
    credit: WindowCredit,
    buffered_bytes: usize,
}

#[derive(Debug)]
pub struct PriorityScheduler {
    queues: [VecDeque<StreamId>; 8],
    consecutive_high_priority: u8,
    lower_priority_cursor: usize,
}

impl Default for PriorityScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl PriorityScheduler {
    pub fn new() -> Self {
        Self {
            queues: core::array::from_fn(|_| VecDeque::new()),
            consecutive_high_priority: 0,
            lower_priority_cursor: 1,
        }
    }

    pub fn enqueue(&mut self, priority: PriorityClass, stream_id: StreamId) {
        self.queues[priority.get() as usize].push_back(stream_id);
    }

    #[must_use]
    pub fn dequeue_next(&mut self) -> Option<StreamId> {
        if self.queues[0].is_empty() {
            self.consecutive_high_priority = 0;
            return self.dequeue_from_lower_priorities();
        }

        let lower_pending = self.queues[1..].iter().any(|q| !q.is_empty());
        if lower_pending && self.consecutive_high_priority >= HIGH_PRIORITY_BURST_LIMIT {
            self.consecutive_high_priority = 0;
            if let Some(id) = self.dequeue_from_lower_priorities() {
                return Some(id);
            }
        }

        let id = self.queues[0].pop_front()?;
        self.consecutive_high_priority = self.consecutive_high_priority.saturating_add(1);
        Some(id)
    }

    fn dequeue_from_lower_priorities(&mut self) -> Option<StreamId> {
        let lower_count = self.queues.len() - 1;
        for offset in 0..lower_count {
            let idx = 1 + ((self.lower_priority_cursor - 1 + offset) % lower_count);
            if let Some(id) = self.queues[idx].pop_front() {
                self.lower_priority_cursor = if idx + 1 >= self.queues.len() {
                    1
                } else {
                    idx + 1
                };
                return Some(id);
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct MuxSessionState {
    authenticated: bool,
    streams: BTreeMap<StreamId, StreamContext>,
    scheduler: PriorityScheduler,
    max_streams: usize,
    max_frame_payload_bytes: usize,
    max_buffered_bytes_per_stream: usize,
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    keepalive: KeepalivePolicy,
    last_peer_activity_tick: Tick,
    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    last_keepalive_ping_tick: Tick,
    current_tick: Tick,
}

#[derive(Debug)]
pub struct MuxHostEndpoint {
    session: MuxSessionState,
    outbound: VecDeque<MuxWireEvent>,
    accepted: VecDeque<AcceptedStream>,
    stream_names_by_id: BTreeMap<StreamId, StreamName>,
    stream_ids_by_name: BTreeMap<String, StreamId>,
}

impl MuxHostEndpoint {
    pub fn new_authenticated(now_tick: Tick) -> Self {
        Self {
            session: MuxSessionState::new_authenticated(now_tick),
            outbound: VecDeque::new(),
            accepted: VecDeque::new(),
            stream_names_by_id: BTreeMap::new(),
            stream_ids_by_name: BTreeMap::new(),
        }
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub fn new_unauthenticated(now_tick: Tick) -> Self {
        Self {
            session: MuxSessionState::new_unauthenticated(now_tick),
            outbound: VecDeque::new(),
            accepted: VecDeque::new(),
            stream_names_by_id: BTreeMap::new(),
            stream_ids_by_name: BTreeMap::new(),
        }
    }

    pub fn open_stream(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
        name: StreamName,
        initial_credit: WindowCredit,
    ) -> Result<(), MuxReject> {
        if self.stream_ids_by_name.contains_key(name.as_str()) {
            return Err(MuxReject::new(REJECT_DUPLICATE_STREAM_NAME));
        }
        self.session
            .open_stream(stream_id, priority, initial_credit)?;
        self.stream_ids_by_name
            .insert(name.as_str().to_string(), stream_id);
        self.stream_names_by_id.insert(stream_id, name.clone());
        self.outbound.push_back(MuxWireEvent::Open {
            stream_id,
            priority,
            name,
        });
        Ok(())
    }

    pub fn send_data(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
        payload_len: usize,
    ) -> Result<SendBudgetOutcome, MuxReject> {
        let outcome = self.session.send_data(stream_id, payload_len)?;
        if matches!(outcome, SendBudgetOutcome::Sent { .. }) {
            self.outbound.push_back(MuxWireEvent::Data {
                stream_id,
                priority,
                payload_len,
            });
        }
        Ok(outcome)
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub fn close_stream(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
    ) -> Result<TransitionOutcome, MuxReject> {
        let outcome = self
            .session
            .apply_transition(stream_id, StreamTransition::SendClose)?;
        self.outbound.push_back(MuxWireEvent::Close {
            stream_id,
            priority,
        });
        Ok(outcome)
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub fn reset_stream(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
    ) -> Result<TransitionOutcome, MuxReject> {
        let outcome = self
            .session
            .apply_transition(stream_id, StreamTransition::Reset)?;
        self.outbound.push_back(MuxWireEvent::Rst {
            stream_id,
            priority,
        });
        Ok(outcome)
    }

    pub fn accept_stream(&mut self) -> Option<AcceptedStream> {
        self.accepted.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<MuxWireEvent> {
        self.outbound.drain(..).collect()
    }

    pub fn ingest(&mut self, event: MuxWireEvent) -> Result<FrameApplyOutcome, MuxReject> {
        match event {
            MuxWireEvent::Open {
                stream_id,
                priority,
                name,
            } => {
                if self.stream_ids_by_name.contains_key(name.as_str()) {
                    return Err(MuxReject::new(REJECT_DUPLICATE_STREAM_NAME));
                }
                let outcome =
                    self.session
                        .apply_inbound_frame(stream_id, priority, InboundFrame::Open)?;
                self.stream_ids_by_name
                    .insert(name.as_str().to_string(), stream_id);
                self.stream_names_by_id.insert(stream_id, name.clone());
                self.accepted.push_back(AcceptedStream {
                    stream_id,
                    priority,
                    name,
                });
                self.outbound.push_back(MuxWireEvent::OpenAck {
                    stream_id,
                    priority,
                });
                Ok(outcome)
            }
            MuxWireEvent::OpenAck {
                stream_id,
                priority,
            } => self
                .session
                .apply_inbound_frame(stream_id, priority, InboundFrame::OpenAck),
            MuxWireEvent::Data {
                stream_id,
                priority,
                payload_len,
            } => self.session.apply_inbound_frame(
                stream_id,
                priority,
                InboundFrame::Data { payload_len },
            ),
            MuxWireEvent::WindowUpdate {
                stream_id,
                priority,
                delta,
            } => self.session.apply_inbound_frame(
                stream_id,
                priority,
                InboundFrame::WindowUpdate { delta },
            ),
            MuxWireEvent::Rst {
                stream_id,
                priority,
            } => self
                .session
                .apply_inbound_frame(stream_id, priority, InboundFrame::Rst),
            MuxWireEvent::Close {
                stream_id,
                priority,
            } => self
                .session
                .apply_inbound_frame(stream_id, priority, InboundFrame::Close),
            MuxWireEvent::Ping {
                stream_id,
                priority,
            } => {
                let outcome =
                    self.session
                        .apply_inbound_frame(stream_id, priority, InboundFrame::Ping)?;
                self.outbound.push_back(MuxWireEvent::Pong {
                    stream_id,
                    priority,
                });
                Ok(outcome)
            }
            MuxWireEvent::Pong {
                stream_id,
                priority,
            } => self
                .session
                .apply_inbound_frame(stream_id, priority, InboundFrame::Pong),
        }
    }

    pub fn stream_state(&self, stream_id: StreamId) -> Option<StreamState> {
        self.session.stream_state(stream_id)
    }

    pub fn buffered_bytes(&self, stream_id: StreamId) -> Option<usize> {
        self.session.stream_buffered_bytes(stream_id)
    }
}

impl MuxSessionState {
    pub fn new_authenticated(now_tick: Tick) -> Self {
        Self::new(true, now_tick)
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub fn new_unauthenticated(now_tick: Tick) -> Self {
        Self::new(false, now_tick)
    }

    fn new(authenticated: bool, now_tick: Tick) -> Self {
        Self {
            authenticated,
            streams: BTreeMap::new(),
            scheduler: PriorityScheduler::new(),
            max_streams: MAX_STREAM_COUNT,
            max_frame_payload_bytes: MAX_FRAME_PAYLOAD_BYTES,
            max_buffered_bytes_per_stream: MAX_BUFFERED_BYTES_PER_STREAM,
            keepalive: KeepalivePolicy::default(),
            last_peer_activity_tick: now_tick,
            last_keepalive_ping_tick: now_tick,
            current_tick: now_tick,
        }
    }

    pub fn open_stream(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
        initial_credit: WindowCredit,
    ) -> Result<(), MuxReject> {
        self.require_authenticated()?;
        if self.streams.len() >= self.max_streams {
            return Err(MuxReject::new(REJECT_STREAM_LIMIT_EXCEEDED));
        }
        if self.streams.contains_key(&stream_id) {
            return Err(MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION));
        }

        let context = StreamContext {
            state: StreamState::Open,
            open_acked: false,
            priority,
            credit: initial_credit,
            buffered_bytes: 0,
        };
        self.streams.insert(stream_id, context);
        self.scheduler.enqueue(priority, stream_id);
        Ok(())
    }

    pub fn apply_transition(
        &mut self,
        stream_id: StreamId,
        transition: StreamTransition,
    ) -> Result<TransitionOutcome, MuxReject> {
        self.require_authenticated()?;
        let context = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| MuxReject::new(REJECT_UNKNOWN_STREAM_FRAME))?;

        let outcome = apply_stream_transition(context.state, transition)?;
        context.state = outcome.next_state;
        Ok(outcome)
    }

    pub fn send_data(
        &mut self,
        stream_id: StreamId,
        payload_len: usize,
    ) -> Result<SendBudgetOutcome, MuxReject> {
        self.require_authenticated()?;
        validate_frame_payload_len(payload_len, self.max_frame_payload_bytes)?;

        let context = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| MuxReject::new(REJECT_UNKNOWN_STREAM_FRAME))?;

        if !matches!(
            context.state,
            StreamState::Open | StreamState::HalfClosedRemote
        ) {
            return Err(MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION));
        }

        let current_credit = context.credit.as_u32() as usize;
        if payload_len > current_credit {
            return Ok(SendBudgetOutcome::WouldBlock {
                remaining_credit: context.credit,
            });
        }

        let projected = context
            .buffered_bytes
            .checked_add(payload_len)
            .ok_or_else(|| MuxReject::new(REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW))?;
        if projected > self.max_buffered_bytes_per_stream {
            return Ok(SendBudgetOutcome::WouldBlock {
                remaining_credit: context.credit,
            });
        }

        let updated_credit = context.credit.as_u32() - payload_len as u32;
        context.credit = WindowCredit::new(updated_credit);
        context.buffered_bytes = projected;
        self.scheduler.enqueue(context.priority, stream_id);

        Ok(SendBudgetOutcome::Sent {
            remaining_credit: context.credit,
        })
    }

    pub fn apply_window_update(
        &mut self,
        stream_id: StreamId,
        delta: i64,
    ) -> Result<WindowCredit, MuxReject> {
        self.require_authenticated()?;
        let context = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| MuxReject::new(REJECT_UNKNOWN_STREAM_FRAME))?;
        let next = apply_window_delta(context.credit, delta)?;
        context.credit = next;
        Ok(next)
    }

    pub fn observe_peer_activity(&mut self, now_tick: Tick) {
        self.last_peer_activity_tick = now_tick;
    }

    pub fn apply_inbound_frame(
        &mut self,
        stream_id: StreamId,
        priority: PriorityClass,
        frame: InboundFrame,
    ) -> Result<FrameApplyOutcome, MuxReject> {
        self.require_authenticated()?;
        match frame {
            InboundFrame::Open => {
                self.open_stream(
                    stream_id,
                    priority,
                    WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT),
                )?;
                Ok(FrameApplyOutcome::StreamOpened {
                    state: StreamState::Open,
                })
            }
            InboundFrame::OpenAck => {
                let context = self
                    .streams
                    .get_mut(&stream_id)
                    .ok_or_else(|| MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION))?;
                if context.open_acked || !matches!(context.state, StreamState::Open) {
                    return Err(MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION));
                }
                context.open_acked = true;
                Ok(FrameApplyOutcome::OpenAcked {
                    state: context.state,
                })
            }
            InboundFrame::Data { payload_len } => {
                validate_frame_payload_len(payload_len, self.max_frame_payload_bytes)?;
                let (buffered_bytes, remaining_credit) = {
                    let context = self
                        .streams
                        .get_mut(&stream_id)
                        .ok_or_else(|| MuxReject::new(REJECT_UNKNOWN_STREAM_FRAME))?;
                    if !matches!(
                        context.state,
                        StreamState::Open | StreamState::HalfClosedLocal
                    ) {
                        return Err(MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION));
                    }
                    let projected =
                        context
                            .buffered_bytes
                            .checked_add(payload_len)
                            .ok_or_else(|| {
                                MuxReject::new(REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW)
                            })?;
                    if projected > self.max_buffered_bytes_per_stream {
                        return Err(MuxReject::new(REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW));
                    }
                    context.buffered_bytes = projected;
                    (projected, context.credit)
                };
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::DataAccepted {
                    buffered_bytes,
                    remaining_credit,
                })
            }
            InboundFrame::WindowUpdate { delta } => {
                let credit = self.apply_window_update(stream_id, delta)?;
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::WindowUpdated { credit })
            }
            InboundFrame::Rst => {
                let transition = self.apply_transition(stream_id, StreamTransition::Reset)?;
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::StreamTransitioned {
                    state: transition.next_state,
                })
            }
            InboundFrame::Close => {
                let transition =
                    self.apply_transition(stream_id, StreamTransition::ReceiveClose)?;
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::StreamTransitioned {
                    state: transition.next_state,
                })
            }
            InboundFrame::Ping => {
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::PingResponseRequired)
            }
            InboundFrame::Pong => {
                self.observe_peer_activity(self.current_tick);
                Ok(FrameApplyOutcome::KeepaliveObserved)
            }
        }
    }

    #[cfg_attr(nexus_env = "os", allow(dead_code))]
    pub fn keepalive_tick(&mut self, now_tick: Tick) -> KeepaliveVerdict {
        self.current_tick = now_tick;
        let idle_ticks = now_tick.saturating_sub(self.last_peer_activity_tick);
        if idle_ticks >= self.keepalive.timeout_ticks {
            return KeepaliveVerdict::TimedOut;
        }

        let since_last_ping = now_tick.saturating_sub(self.last_keepalive_ping_tick);
        if since_last_ping >= self.keepalive.interval_ticks {
            self.last_keepalive_ping_tick = now_tick;
            return KeepaliveVerdict::SendPing;
        }

        KeepaliveVerdict::Healthy
    }

    #[must_use]
    pub fn dequeue_next_stream(&mut self) -> Option<StreamId> {
        self.scheduler.dequeue_next()
    }

    pub fn stream_state(&self, stream_id: StreamId) -> Option<StreamState> {
        self.streams.get(&stream_id).map(|ctx| ctx.state)
    }

    pub fn stream_buffered_bytes(&self, stream_id: StreamId) -> Option<usize> {
        self.streams.get(&stream_id).map(|ctx| ctx.buffered_bytes)
    }

    fn require_authenticated(&self) -> Result<(), MuxReject> {
        if self.authenticated {
            Ok(())
        } else {
            Err(MuxReject::new(REJECT_UNAUTHENTICATED_SESSION))
        }
    }
}

pub fn validate_frame_payload_len(payload_len: usize, max_payload: usize) -> Result<(), MuxReject> {
    if payload_len > max_payload {
        Err(MuxReject::new(REJECT_FRAME_OVERSIZE))
    } else {
        Ok(())
    }
}

pub fn apply_window_delta(current: WindowCredit, delta: i64) -> Result<WindowCredit, MuxReject> {
    if delta >= 0 {
        let next = current
            .as_u32()
            .checked_add(delta as u32)
            .ok_or_else(|| MuxReject::new(REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW))?;
        Ok(WindowCredit::new(next))
    } else {
        let debit = (-delta) as u32;
        let next = current
            .as_u32()
            .checked_sub(debit)
            .ok_or_else(|| MuxReject::new(REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW))?;
        Ok(WindowCredit::new(next))
    }
}

pub fn apply_stream_transition(
    state: StreamState,
    transition: StreamTransition,
) -> Result<TransitionOutcome, MuxReject> {
    let next_state = match (state, transition) {
        (StreamState::Open, StreamTransition::SendClose) => StreamState::HalfClosedLocal,
        (StreamState::Open, StreamTransition::ReceiveClose) => StreamState::HalfClosedRemote,
        (StreamState::HalfClosedLocal, StreamTransition::ReceiveClose) => StreamState::Closed,
        (StreamState::HalfClosedRemote, StreamTransition::SendClose) => StreamState::Closed,
        (
            StreamState::Open | StreamState::HalfClosedLocal | StreamState::HalfClosedRemote,
            StreamTransition::Reset,
        ) => StreamState::Reset,
        (StreamState::Reset, StreamTransition::Reset) => StreamState::Reset,
        _ => return Err(MuxReject::new(REJECT_INVALID_STREAM_STATE_TRANSITION)),
    };
    Ok(TransitionOutcome { next_state })
}
