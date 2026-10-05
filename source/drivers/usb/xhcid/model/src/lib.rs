// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The machine xhcid is proven against on the host (TASK-0328 U1): an xHCI controller
//! (`ctrl`, `exec` — registers, command/transfer/event rings, port changes), the devices on its
//! ports (`dev` — HID boot devices and hubs answering their requests from descriptor bytes), and
//! DMA memory behind a non-coherent cache (`dma-model`, every driver model's memory). The
//! controller reads every TRB, context and table from DRAM by bus address, so a driver that
//! forgot to publish a write hands it stale memory, and one that forgot to observe reads stale
//! memory itself — the test shows either. [`Rig`] drives a driver against it deterministically:
//! interrupts first, then time jumps to the driver's next deadline, until nothing waits.
//! Host only: no OS crate depends on it.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: xhcid's `tests/xhci` prove the machine against the driver

#![forbid(unsafe_code)]
// A test machine, host only: a fixture that cannot be built is a failed test, reported where it
// broke — its inputs are the tests' own constants, never untrusted data.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod ctrl;
pub mod dev;
mod exec;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dma_model::{Dram, HasDram, ModelCache, ModelMem};
use nexus_abi::DmaCoherence;
use nexus_driverkit::DmaShared;
use nexus_hal::Bus;
use nexus_usb::HidRole;
use xhcid::{Channel, DmaAlloc, HidClass, Note, Pushed, Sink, Step, Xhci};

pub use ctrl::{Config, Faults, RootPort};
pub use dev::Dev;

/// The board's cache block: every region is maintained (the SoC bus is non-coherent).
pub const NONCOHERENT: DmaCoherence = DmaCoherence::Maintained { block: 64 };

/// The machine.
pub struct Machine {
    pub hc: ctrl::Hc,
    pub mem: Dram,
    pub ports: Vec<RootPort>,
}

impl HasDram for Machine {
    fn dram(&mut self) -> &mut Dram {
        &mut self.mem
    }
}

/// The machine, shared by the bus, the memory and the test.
pub type Shared = Rc<RefCell<Machine>>;

/// A machine of the given shape, nothing plugged in.
#[must_use]
pub fn machine(config: Config) -> Shared {
    let ports = config
        .revisions
        .iter()
        .map(|&revision| RootPort { revision, pp: !config.port_power, ..RootPort::default() })
        .collect();
    Rc::new(RefCell::new(Machine { hc: ctrl::Hc::new(config), mem: Dram::default(), ports }))
}

/// The registers.
pub struct ModelBus(pub Shared);

impl Bus for ModelBus {
    fn read(&self, addr: usize) -> u32 {
        assert_eq!(addr % 4, 0, "aligned words only");
        self.0.borrow_mut().read(addr)
    }
    fn write(&self, addr: usize, value: u32) {
        assert_eq!(addr % 4, 0, "aligned words only");
        self.0.borrow_mut().write(addr, value);
    }
}

/// The driver's DMA memory: contiguous regions at increasing bus addresses, each aligned to
/// its size, behind the non-coherent cache.
pub struct ModelAlloc {
    pub m: Shared,
    next: u64,
}

impl DmaAlloc for ModelAlloc {
    type Mem = ModelMem;
    type Cache = ModelCache<Machine>;

    fn shared(&mut self, len: usize) -> Option<DmaShared<ModelMem, ModelCache<Machine>>> {
        let bus = self.next.next_multiple_of(len as u64);
        self.next = bus + len as u64;
        let mem = ModelMem::new(&self.m, len, bus, 1, 0);
        DmaShared::new(mem, NONCOHERENT, ModelCache(self.m.clone())).ok()
    }
}

/// A note as the test keeps it (a report's bytes copied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    /// Any note but a report, as its debug text.
    Note(String),
    /// A report: slot, interface, role, bytes.
    Report(u8, u8, HidRole, Vec<u8>),
}

/// The HID class subscriber's push channel as a test sees it (TASK-0253B): every frame it
/// took, and switches that make it full (the client is not draining) or gone (it died). The
/// class owns the channel, so the test keeps a clone of the shared handles.
#[derive(Clone, Default)]
pub struct Frames {
    pub taken: Rc<RefCell<Vec<Vec<u8>>>>,
    pub full: Rc<Cell<bool>>,
    pub gone: Rc<Cell<bool>>,
}

impl Frames {
    /// The frames taken so far, and forgets them (a test that only forgets ignores the list).
    pub fn drain(&self) -> Vec<Vec<u8>> {
        core::mem::take(&mut *self.taken.borrow_mut())
    }
}

impl Channel for Frames {
    fn push(&mut self, frame: &[u8]) -> Pushed {
        if self.gone.get() {
            return Pushed::Gone;
        }
        if self.full.get() {
            return Pushed::Full;
        }
        self.taken.borrow_mut().push(frame.to_vec());
        Pushed::Sent
    }
}

/// Everything the driver noted — and, as the OS loop does, every note handed to the HID class
/// server too (TASK-0253B), whose subscriber is a [`Frames`] when a test subscribes one.
#[derive(Default)]
pub struct Recorder {
    pub seen: Vec<Seen>,
    pub fails: Vec<(Step, u8)>,
    pub class: HidClass<Frames>,
}

impl Sink for Recorder {
    fn note(&mut self, note: Note<'_>) {
        self.class.note(&note);
        match note {
            Note::Report { slot, interface, role, bytes } => {
                self.seen.push(Seen::Report(slot, interface, role, bytes.to_vec()));
            }
            Note::Fail { step, code } => {
                self.fails.push((step, code));
                self.seen.push(Seen::Note(format!("{note:?}")));
            }
            other => self.seen.push(Seen::Note(format!("{other:?}"))),
        }
    }
}

impl Recorder {
    /// The notes whose text starts with `prefix` (e.g. `"Enumerated"`).
    #[must_use]
    pub fn notes(&self, prefix: &str) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|s| match s {
                Seen::Note(n) if n.starts_with(prefix) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// The reports, in order.
    #[must_use]
    pub fn reports(&self) -> Vec<(u8, u8, HidRole, Vec<u8>)> {
        self.seen
            .iter()
            .filter_map(|s| match s {
                Seen::Report(a, b, c, d) => Some((*a, *b, *c, d.clone())),
                Seen::Note(_) => None,
            })
            .collect()
    }
}

/// A driver on a machine.
pub struct Rig {
    pub m: Shared,
    pub xhci: Xhci<ModelBus, ModelAlloc>,
    pub sink: Recorder,
    pub now: u64,
}

impl Rig {
    /// A driver over `m` (not started).
    #[must_use]
    pub fn new(m: &Shared) -> Self {
        let alloc = ModelAlloc { m: m.clone(), next: 0x8000_0000 };
        Self {
            m: m.clone(),
            xhci: Xhci::new(ModelBus(m.clone()), alloc),
            sink: Recorder::default(),
            now: 0,
        }
    }

    /// Start the driver and run until nothing waits.
    pub fn start(&mut self) {
        self.xhci.start(self.now, &mut self.sink);
        self.settle();
    }

    /// Run until no interrupt is pending and no deadline is left (bounded). After every wake
    /// the HID class flushes what it owes, as the OS loop does; its retry deadline is left to
    /// the test ([`Self::retry`]) — a client that never drains would never let it settle.
    pub fn settle(&mut self) {
        for _ in 0..1_000_000 {
            if self.m.borrow().irq() {
                self.xhci.on_interrupt(self.now, &mut self.sink);
                self.sink.class.flush(self.now);
                continue;
            }
            match self.xhci.deadline() {
                Some(at) => {
                    self.now = self.now.max(at);
                    self.xhci.on_timer(self.now, &mut self.sink);
                    self.sink.class.flush(self.now);
                }
                None => return,
            }
        }
        panic!("the driver never settled");
    }

    /// Subscribes a fresh [`Frames`] to the HID class (as an admitted SUBSCRIBE does) and
    /// flushes: the answer and every interface attached so far go out. Returns the handles.
    pub fn subscribe(&mut self) -> Frames {
        let frames = Frames::default();
        drop(self.sink.class.subscribe(frames.clone()));
        self.sink.class.flush(self.now);
        frames
    }

    /// Time moves to the HID class's retry deadline (if any), which fires.
    pub fn retry(&mut self) {
        if let Some(at) = self.sink.class.deadline() {
            self.now = self.now.max(at);
            self.sink.class.on_timer(self.now);
        }
    }

    /// A report from the device at (`root`, `route`) on endpoint `address`, then settle.
    pub fn report(&mut self, root: u8, route: u32, address: u8, bytes: &[u8]) {
        self.m.borrow_mut().report(root, route, address, bytes);
        self.settle();
    }
}
