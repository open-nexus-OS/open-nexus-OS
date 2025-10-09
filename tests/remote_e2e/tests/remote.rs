#![cfg(nexus_env = "host")]

use dsoftbus::Announcement;
use remote_e2e::{random_port, Node};

const MANIFEST: &str = r#"name = "demo"
version = "1.0.0"
abilities = ["ui"]
caps = ["gpu"]
min_sdk = "0.1.0"
signature = "valid"
"#;

#[test]
fn remote_roundtrip_and_negative_handshake() {
    let port_b = random_port();
    let node_b = Node::start(port_b, vec!["samgrd".into(), "bundlemgrd".into()]).unwrap();
    node_b.register_service("bundlemgrd", 7).expect("register bundlemgrd");

    let port_a = random_port();
    let node_a = Node::start(port_a, vec!["samgrd".into()]).unwrap();

    eprintln!("[remote_e2e] waiting for node B announcement");
    let mut announcements = node_a.watch().expect("watch registry");
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
    connection.push_artifact(handle, MANIFEST.as_bytes()).expect("upload artifact");
    assert!(connection
        .install_bundle("demo", handle, MANIFEST.len() as u32)
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
