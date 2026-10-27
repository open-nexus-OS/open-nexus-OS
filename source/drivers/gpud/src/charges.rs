// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU memory charged to its client (TASK-0333 6e-4, RFC-0100 §3, ADR-0078/0086). The
//! GPU's pool memory is nobody's by itself: every byte gpud holds, it holds FOR a client — the
//! framebuffer object it made and lent (the scan-out planes, the atlas' reservation; ADR-0078),
//! the buffers of the client's GPU context on the board (`GpuService::held_by`). This table says
//! what each client costs, by kernel service id, as an absolute figure per part; a change is
//! told to the one memory authority (`nexus_wire::memd::OP_GPU_CHARGE`, NONBLOCK — gpud never
//! waits on memd; the next change tells the truth) and named: `gpud: gpu memory charged
//! (client= bytes= told=)`. Bounded: eight clients (the compositor and the system UI today; app
//! surfaces of their own are layer 8's); a ninth is refused by name, never silently uncounted.
//! What gpud DROPS at `warn` stands on this ledger — a cache costs whom — and follows.
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below (a charge replaces, parts sum, zero frees the row, the bound);
//!   the display lanes' `gpud: gpu memory charged (client=windowd` → `memd: gpu charge (client=`

/// Clients the table holds at once.
pub(crate) const MAX_CLIENTS: usize = 8;

/// What a client's charge is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The framebuffer object gpud made and lent the client (its whole span: a lazy object's
    /// reservation counts whole — it may all be drawn).
    Lent,
    /// The buffers of the client's GPU context the device holds (the board).
    Context,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Row {
    client: u64,
    lent: u64,
    context: u64,
}

impl Row {
    const fn total(&self) -> u64 {
        self.lent.saturating_add(self.context)
    }
}

/// What setting a part did: the client's new total, when it changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Charged {
    /// The client's total is now `bytes` (a row freed answers 0).
    Changed(u64),
    /// The figure was the one already held.
    Unchanged,
    /// A ninth client: refused, nothing counted.
    Full,
}

/// Every client's charge.
pub(crate) struct Charges {
    rows: [Option<Row>; MAX_CLIENTS],
}

impl Charges {
    pub(crate) const fn new() -> Self {
        Self { rows: [None; MAX_CLIENTS] }
    }

    /// `client`'s `part` is `bytes` now (absolute). A client whose every part is zero leaves
    /// the table.
    pub(crate) fn set(&mut self, client: u64, part: Part, bytes: u64) -> Charged {
        let slot = match self.rows.iter().position(|r| r.is_some_and(|r| r.client == client)) {
            Some(i) => i,
            None if bytes == 0 => return Charged::Unchanged,
            None => match self.rows.iter().position(Option::is_none) {
                Some(i) => {
                    self.rows[i] = Some(Row { client, ..Row::default() });
                    i
                }
                None => return Charged::Full,
            },
        };
        let Some(row) = self.rows[slot].as_mut() else { return Charged::Full };
        let field = match part {
            Part::Lent => &mut row.lent,
            Part::Context => &mut row.context,
        };
        if *field == bytes {
            return Charged::Unchanged;
        }
        *field = bytes;
        let total = row.total();
        if total == 0 {
            self.rows[slot] = None;
        }
        Charged::Changed(total)
    }

    /// What `client` costs now.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn of(&self, client: u64) -> u64 {
        self.rows.iter().flatten().find(|r| r.client == client).map_or(0, Row::total)
    }

    /// What every client costs together.
    #[must_use]
    pub(crate) fn total(&self) -> u64 {
        self.rows.iter().flatten().map(Row::total).fold(0, u64::saturating_add)
    }
}

/// The client's name for the console: the compositor by name, anyone else by id.
pub(crate) fn client_name(client: u64) -> ClientName {
    if client == nexus_abi::service_id_from_name(b"windowd") {
        ClientName::Windowd
    } else {
        ClientName::Id(client)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientName {
    Windowd,
    Id(u64),
}

impl core::fmt::Display for ClientName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Windowd => f.write_str("windowd"),
            Self::Id(id) => write!(f, "{id:#x}"),
        }
    }
}

/// The leg to the memory authority (the OS alone): the push, and the console line.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) struct Teller {
    memd: Option<nexus_ipc::KernelClient>,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl Teller {
    pub(crate) fn bind() -> Self {
        let leg = nexus_service_topology::slots::gpud::MEMD;
        Self { memd: nexus_ipc::KernelClient::new_with_slots(leg.send, leg.recv).ok() }
    }

    /// `client`'s `part` set to `bytes`; a changed total is told to memd and named.
    pub(crate) fn charge(&self, charges: &mut Charges, client: u64, part: Part, bytes: u64) {
        use nexus_ipc::Client as _;
        let total = match charges.set(client, part, bytes) {
            Charged::Changed(total) => total,
            Charged::Unchanged => return,
            Charged::Full => {
                let _ = nexus_abi::debug_println(&alloc::format!(
                    "gpud: FAIL gpu memory charge (client={} table full)",
                    client_name(client)
                ));
                return;
            }
        };
        let frame = nexus_wire::memd::encode_gpu_charge(client, total);
        let told = self
            .memd
            .as_ref()
            .is_some_and(|c| c.send(&frame, nexus_ipc::Wait::NonBlocking).is_ok());
        let _ = nexus_abi::debug_println(&alloc::format!(
            "gpud: gpu memory charged (client={} bytes={total} all={} told={})",
            client_name(client),
            charges.total(),
            u8::from(told)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWD: u64 = 0x11;
    const SHELL: u64 = 0x22;

    #[test]
    fn a_charge_replaces_and_the_parts_sum() {
        let mut c = Charges::new();
        assert_eq!(c.set(WINDOWD, Part::Lent, 40 << 20), Charged::Changed(40 << 20));
        assert_eq!(c.set(WINDOWD, Part::Lent, 40 << 20), Charged::Unchanged, "the same figure");
        assert_eq!(c.set(WINDOWD, Part::Context, 6 << 20), Charged::Changed(46 << 20));
        assert_eq!(c.set(WINDOWD, Part::Lent, 48 << 20), Charged::Changed(54 << 20), "absolute");
        assert_eq!(c.of(WINDOWD), 54 << 20);
        assert_eq!(c.set(SHELL, Part::Lent, 1 << 20), Charged::Changed(1 << 20));
        assert_eq!(c.total(), 55 << 20);
    }

    #[test]
    fn zero_in_every_part_frees_the_row_and_a_zero_for_nobody_is_nothing() {
        let mut c = Charges::new();
        assert_eq!(c.set(WINDOWD, Part::Lent, 0), Charged::Unchanged, "never charged");
        c.set(WINDOWD, Part::Lent, 4096);
        c.set(WINDOWD, Part::Context, 4096);
        assert_eq!(c.set(WINDOWD, Part::Lent, 0), Charged::Changed(4096));
        assert_eq!(c.set(WINDOWD, Part::Context, 0), Charged::Changed(0));
        assert_eq!(c.of(WINDOWD), 0);
        assert!(c.rows.iter().all(Option::is_none), "the row is free again");
    }

    #[test]
    fn test_reject_a_ninth_client() {
        let mut c = Charges::new();
        for id in 1..=MAX_CLIENTS as u64 {
            assert_eq!(c.set(id, Part::Lent, 4096), Charged::Changed(4096));
        }
        assert_eq!(c.set(99, Part::Lent, 4096), Charged::Full);
        assert_eq!(c.of(99), 0, "nothing counted for the refused one");
        assert_eq!(c.total(), MAX_CLIENTS as u64 * 4096);
    }

    #[test]
    fn the_compositor_is_named_and_anyone_else_is_an_id() {
        assert_eq!(client_name(nexus_abi::service_id_from_name(b"windowd")), ClientName::Windowd);
        assert_eq!(client_name(0x2a), ClientName::Id(0x2a));
        assert_eq!(alloc::format!("{}", ClientName::Id(0x2a)), "0x2a");
    }
}
