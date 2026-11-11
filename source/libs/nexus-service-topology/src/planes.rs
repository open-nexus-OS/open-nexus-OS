// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The device planes a machine may lack, and the services that run only where the plane
//! exists. Whether a device exists is a fact of the device tree, read once by init — never
//! something a service finds out by trying: init resumes a service listed here only on a machine
//! that has its plane, and grants the plane's window before that resume. A service therefore
//! reads its device slot exactly once; there is no probe, no retry and no deadline to write.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: host tests below + `nexus-init`'s resume-set and barrier tests

use crate::ServiceId;

/// A device plane the device tree may or may not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DevicePlane {
    /// A network device.
    Net = 0,
}

impl DevicePlane {
    /// Every plane.
    pub const ALL: [DevicePlane; 1] = [DevicePlane::Net];

    /// The name init's lines carry (`init: net plane none …`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Net => "net",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The planes a machine has. Built by init from the device tree and by nobody else: a set a
/// service could assemble would be a claim, this one is a reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use]
pub struct Planes(u8);

impl Planes {
    /// A machine with none of the optional planes.
    pub const NONE: Planes = Planes(0);

    /// This set plus `plane`.
    pub const fn with(self, plane: DevicePlane) -> Planes {
        Planes(self.0 | plane.bit())
    }

    /// `true` if the machine has `plane`.
    #[must_use]
    pub const fn has(self, plane: DevicePlane) -> bool {
        self.0 & plane.bit() != 0
    }

    /// The plane `service` needs and this machine lacks, if any.
    #[must_use]
    pub fn missing_for(self, service: ServiceId) -> Option<DevicePlane> {
        needs_plane(service).filter(|plane| !self.has(*plane))
    }
}

/// The services that run only on a machine with the plane: the plane's owner and the services
/// whose whole job stands on it. A client that merely may call the owner (the proof harness) is
/// not listed — it runs everywhere and skips what the machine lacks.
pub const PLANE_NEEDS: &[(ServiceId, DevicePlane)] = &[
    (ServiceId::Netstackd, DevicePlane::Net), // the plane's owner
    (ServiceId::Ingressd, DevicePlane::Net),  // exposes listeners on the network
    (ServiceId::Dsoftbusd, DevicePlane::Net), // the distributed bus over the network
];

/// The plane `service` needs, if it needs one.
#[must_use]
pub fn needs_plane(service: ServiceId) -> Option<DevicePlane> {
    PLANE_NEEDS.iter().find(|(svc, _)| *svc == service).map(|(_, plane)| *plane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_needs_at_most_one_plane() {
        for (i, (svc, _)) in PLANE_NEEDS.iter().enumerate() {
            assert!(
                PLANE_NEEDS[i + 1..].iter().all(|(other, _)| other != svc),
                "{} is listed twice",
                svc.name()
            );
        }
    }

    #[test]
    fn a_machine_without_the_plane_lacks_it_for_exactly_the_listed_services() {
        for svc in ServiceId::ALL {
            let listed = PLANE_NEEDS.iter().find(|(s, _)| *s == svc).map(|(_, p)| *p);
            assert_eq!(Planes::NONE.missing_for(svc), listed, "{}", svc.name());
        }
    }

    #[test]
    fn a_machine_with_the_plane_lacks_nothing() {
        let all = DevicePlane::ALL.iter().fold(Planes::NONE, |set, plane| set.with(*plane));
        for svc in ServiceId::ALL {
            assert_eq!(all.missing_for(svc), None, "{}", svc.name());
        }
        for plane in DevicePlane::ALL {
            assert!(all.has(plane) && !Planes::NONE.has(plane));
        }
    }

    /// The owner of a plane is called by the services standing on it: a listed non-owner without
    /// a declared route to a listed service would be withheld for a plane it never uses.
    #[test]
    fn test_reject_a_plane_need_without_a_route_into_the_plane() {
        for (svc, plane) in PLANE_NEEDS {
            let owner = PLANE_NEEDS.iter().find(|(_, p)| p == plane).map(|(s, _)| *s);
            if owner == Some(*svc) {
                continue;
            }
            let reaches = crate::REQUIRED_ROUTES.iter().any(|(from, to)| {
                from == svc && PLANE_NEEDS.iter().any(|(s, p)| s == to && p == plane)
            });
            assert!(reaches, "{} needs {} but calls nobody on it", svc.name(), plane.name());
        }
    }
}
