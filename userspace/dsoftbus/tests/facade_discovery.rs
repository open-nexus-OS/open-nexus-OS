// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for DSoftBus discovery over nexus-net sockets facade (FakeNet UDP)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 integration tests
//!
//! TEST_SCOPE:
//!   - Discovery announce + watch over a UDP sockets facade backend (FakeNet)
//!
//! TEST_SCENARIOS:
//!   - facade_discovery_announce_and_watch(): receiver sees announced peer deterministically
//!   - facade_discovery_watch_seeds_cache(): watch yields cached peers without re-announce
//!   - facade_discovery_multi_announce_yields_all_peers(): watcher yields multiple peers deterministically
//!
//! DEPENDENCIES:
//!   - dsoftbus::FacadeDiscovery
//!   - nexus_net::fake::FakeNet
//!   - identity::Identity
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(nexus_env = "host")]
mod host {
    use std::net::SocketAddr;

    use dsoftbus::{Announcement, Discovery, FacadeDiscovery};
    use identity::Identity;
    use nexus_net::fake::FakeNet;

    #[test]
    fn facade_discovery_announce_and_watch() {
        let net = FakeNet::new();
        let bus = SocketAddr::from(([127, 0, 0, 1], 37020));

        let identity_a = Identity::generate().expect("identity a");
        let ann_a = Announcement::new(
            identity_a.device_id().clone(),
            vec!["samgrd".to_string(), "bundlemgrd".to_string()],
            1234,
            [0x22; 32],
        );

        let disc_a = FacadeDiscovery::new(net.clone(), bus, bus).expect("disc a");
        let disc_b = FacadeDiscovery::new(net, bus, bus).expect("disc b");

        // Nothing cached before receiving.
        assert!(disc_b.get(identity_a.device_id()).unwrap().is_none());

        disc_a.announce(ann_a.clone()).expect("announce");

        let mut stream = disc_b.watch().expect("watch");
        let got = stream.next().expect("expected announcement");
        assert_eq!(got.device_id(), ann_a.device_id());
        assert_eq!(got.services(), ann_a.services());
        assert_eq!(got.port(), ann_a.port());
        assert_eq!(got.noise_static(), ann_a.noise_static());

        // Cache now contains the peer.
        assert!(disc_b.get(identity_a.device_id()).unwrap().is_some());
    }

    #[test]
    fn facade_discovery_watch_seeds_cache() {
        let net = FakeNet::new();
        let bus = SocketAddr::from(([127, 0, 0, 1], 37021));

        let identity_a = Identity::generate().expect("identity a");
        let ann_a = Announcement::new(
            identity_a.device_id().clone(),
            vec!["samgrd".to_string()],
            2222,
            [0x33; 32],
        );

        let disc_a = FacadeDiscovery::new(net.clone(), bus, bus).expect("disc a");
        let disc_b = FacadeDiscovery::new(net, bus, bus).expect("disc b");

        disc_a.announce(ann_a.clone()).expect("announce");
        // First watch consumes the packet and seeds the cache.
        let mut w1 = disc_b.watch().expect("watch1");
        let _ = w1.next().expect("expected announcement");
        assert!(disc_b.get(identity_a.device_id()).unwrap().is_some());

        // Second watch should yield from cache immediately, without requiring a new announce.
        let mut w2 = disc_b.watch().expect("watch2");
        let got = w2.next().expect("expected cached announcement");
        assert_eq!(got.device_id(), ann_a.device_id());
        assert_eq!(got.port(), ann_a.port());
    }

    #[test]
    fn facade_discovery_multi_announce_yields_all_peers() {
        let net = FakeNet::new();
        let bus = SocketAddr::from(([127, 0, 0, 1], 37022));

        let identity_a = Identity::generate().expect("identity a");
        let identity_b = Identity::generate().expect("identity b");

        let ann_a = Announcement::new(
            identity_a.device_id().clone(),
            vec!["samgrd".to_string()],
            1111,
            [0x44; 32],
        );
        let ann_b = Announcement::new(
            identity_b.device_id().clone(),
            vec!["bundlemgrd".to_string()],
            2222,
            [0x55; 32],
        );

        let disc_a = FacadeDiscovery::new(net.clone(), bus, bus).expect("disc a");
        let disc_b = FacadeDiscovery::new(net.clone(), bus, bus).expect("disc b");
        let disc_w = FacadeDiscovery::new(net, bus, bus).expect("watcher");

        disc_a.announce(ann_a.clone()).expect("announce a");
        disc_b.announce(ann_b.clone()).expect("announce b");

        let mut w = disc_w.watch().expect("watch");
        let first = w.next().expect("first");
        let second = w.next().expect("second");

        let mut got =
            vec![first.device_id().as_str().to_string(), second.device_id().as_str().to_string()];
        got.sort();

        let mut expected =
            vec![ann_a.device_id().as_str().to_string(), ann_b.device_id().as_str().to_string()];
        expected.sort();

        assert_eq!(got, expected);
    }
}
