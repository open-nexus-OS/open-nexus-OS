//! CONTEXT: Remote end-to-end integration tests (host-first complement to tools/os2vm.sh)
//! INTENT: Distributed service discovery, remote bundle management, authentication
//! IDL (target): announce(device_id,services,port), connect(remote), resolve(name), installBundle(name,handle,len)
//! DEPS: dsoftbusd, samgrd, bundlemgrd (distributed service integration)
//! READINESS: Multiple nodes started; service discovery active
//! TESTS: Service discovery, remote resolution, bundle install, authentication failure
//!
//! This test suite exercises the DSoftBus-lite stack (TASK-0005 / RFC-0010) entirely
//! in-process using FakeNet. It provides fast, deterministic verification of the
//! distributed service integration (discovery → Noise XK auth → remote proxy).
//!
//! For OS-level proof with real networking (virtio-net, smoltcp, UDP), see:
//!   - tools/os2vm.sh (2-VM QEMU harness, opt-in via RUN_OS2VM=1)
//!   - TASK-0005: tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
//!   - RFC-0010: docs/rfcs/RFC-0010-dsoftbus-remote-proxy-v1.md
#![cfg(nexus_env = "host")]

use capnp::message::Builder;
use capnp::serialize;
use dsoftbus::Announcement;
use nexus_idl_runtime::manifest_capnp::bundle_manifest;
use nexus_net::fake::FakeNet;
use remote_e2e::{random_port, ArtifactKind, Node};

fn manifest_nxb() -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut m = message.init_root::<bundle_manifest::Builder<'_>>();
        m.set_schema_version(1);
        m.set_name("demo");
        m.set_semver("1.0.0");
        m.set_min_sdk("0.1.0");
        m.set_publisher(&[0xaa; 16]);
        m.set_signature(&[0xaa; 64]);
        let mut abilities = m.reborrow().init_abilities(1);
        abilities.set(0, "ui");
        let mut caps = m.reborrow().init_capabilities(1);
        caps.set(0, "gpu");
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &message).expect("serialize manifest");
    out
}

#[test]
fn remote_roundtrip_and_negative_handshake() {
    let net = FakeNet::new();
    let port_a = random_port();
    let node_a = Node::start_facade(net.clone(), port_a, vec!["samgrd".into()]).unwrap();

    eprintln!("[remote_e2e] watching for node B announcement");
    let mut announcements = node_a.watch().expect("watch registry");

    eprintln!("[remote_e2e] starting node B");
    let port_b = random_port();
    let node_b =
        Node::start_facade(net, port_b, vec!["samgrd".into(), "bundlemgrd".into()]).unwrap();
    node_b.register_service("bundlemgrd", 7).expect("register bundlemgrd");

    let remote =
        announcements.find(|ann| ann.device_id() == &node_b.device_id()).expect("discover node b");
    eprintln!("[remote_e2e] discovered node B at port {}", remote.port());

    // Positive resolve path
    eprintln!("[remote_e2e] connecting to node B");
    let connection = node_a.connect(&remote).expect("connect to node b");
    assert!(connection.resolve("bundlemgrd").expect("remote resolve succeeds"));
    assert!(!connection.resolve("missing-service").expect("missing service resolves to false"));

    // Install bundle through the remote bundle manager
    let handle = 42u32;
    let manifest = manifest_nxb();
    connection.push_artifact(handle, ArtifactKind::Manifest, &manifest).expect("upload manifest");
    connection.push_artifact(handle, ArtifactKind::Payload, &[0x00]).expect("upload payload");
    assert!(connection
        .install_bundle("demo", handle, manifest.len() as u32)
        .expect("remote install ok"));
    let version =
        connection.query_bundle("demo").expect("query remote bundle").expect("bundle installed");
    assert_eq!(version, "1.0.0");

    // Tamper with the static key to ensure authentication fails
    let mut corrupted_static = *remote.noise_static();
    corrupted_static[0] ^= 0xFF;
    let tampered = Announcement::new(
        remote.device_id().clone(),
        remote.services().to_vec(),
        remote.port(),
        corrupted_static,
    );
    assert!(node_a.connect(&tampered).is_err());
}
