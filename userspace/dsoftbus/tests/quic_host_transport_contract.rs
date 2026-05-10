// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Behavior-first host proofs for real QUIC transport contract in TASK-0021
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 6 integration tests (real QUIC path + mux smoke payload + reject/fallback mapping)
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use dsoftbus::{
    build_server_config, probe_and_echo_once, select_transport_with_host_quic, HostQuicProbeError,
    HostQuicProbeRequest, MuxHostEndpoint, MuxWireEvent, PriorityClass, StreamId, StreamName,
    TransportKind, TransportMode, TransportSelectionError, WindowCredit,
    DSOFTBUS_QUIC_DEFAULT_ALPN, MARKER_QUIC_OS_DISABLED_FALLBACK_TCP,
    MARKER_SELFTEST_QUIC_FALLBACK_OK, MARKER_TRANSPORT_SELECTED_QUIC,
    MARKER_TRANSPORT_SELECTED_TCP,
};
use quinn::Endpoint;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ECHO_BYTES: usize = 64 * 1024;
const TEST_SERVER_CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDBTCCAe2gAwIBAgIUUylStkOTc6ehi4754RX5VwWLUXEwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDQxNDEyMDk0MloXDTM2MDQx
MTEyMDk0MlowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAponvyp+DAJscYz9D3Tx/KQgdaSeQVEIPvifx+Gs0O6o0
AAVYKBj6HqKJDaMJfALtwgqVW64+JRrSEgwXGH2sfdc0uIukpg8n0TEhaHKAGagT
t5ZxnbkRavDZHq+VoLZIAsccZsW9rd01hPcaXdFE4fNg9OK+qQdTOsGYMkwo79wM
GQq61QkbcdvdadDHqDac6tHsvoAKzJLrTlc7EATmHgvV5TmteYQ7nrl5bq70bl6L
MtfkvqtxOga4xXZ+FwtyKMTqpsr9cPniH71xeEqVgQF5bHFv/VmWhkdh9cK1z0TU
yZgomILdIZvpCLvf40eGkAE35CjxwMP6SsGy0kc9VwIDAQABo08wTTAJBgNVHRME
AjAAMAsGA1UdDwQEAwIF4DAUBgNVHREEDTALgglsb2NhbGhvc3QwHQYDVR0OBBYE
FF0YN7eit9XLd7mkUDOLGIYH0WEFMA0GCSqGSIb3DQEBCwUAA4IBAQCRgPzUA5Iw
JiqZ+RBC8ZAbzms0ruUzwyo0ckYXTrTGvR1MkNjg3yNAHasjS//5uNthTY0BcT1Y
PZ8b5tG2MbQwJByu7GjpTjwADF9TIO8MPSkYj9SMPjpBLj2XbwZThpu68kBJCN/H
IsQoyqVyg3IB+f4AX9eBljLtgqawnFsJjNgtx+K9IeRR6jitukZ+CgqdfNi8a9g4
qSbuUtgyOhcEL6uCJFCAK7HArXPAB2JZzRhtys9BmybjRHkHjtC4UMg3MtqmTEXz
sYALel6pOgVd8TkokOTtgENPOVcrIxf09wv4QmCLbUDO0H6cXTUxeYYRDWpEAHnM
B9xEzRde6BYR
-----END CERTIFICATE-----
"#;
const TEST_SERVER_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCmie/Kn4MAmxxj
P0PdPH8pCB1pJ5BUQg++J/H4azQ7qjQABVgoGPoeookNowl8Au3CCpVbrj4lGtIS
DBcYfax91zS4i6SmDyfRMSFocoAZqBO3lnGduRFq8Nker5WgtkgCxxxmxb2t3TWE
9xpd0UTh82D04r6pB1M6wZgyTCjv3AwZCrrVCRtx291p0MeoNpzq0ey+gArMkutO
VzsQBOYeC9XlOa15hDueuXlurvRuXosy1+S+q3E6BrjFdn4XC3IoxOqmyv1w+eIf
vXF4SpWBAXlscW/9WZaGR2H1wrXPRNTJmCiYgt0hm+kIu9/jR4aQATfkKPHAw/pK
wbLSRz1XAgMBAAECggEAFK6uvABBWbLpvJ2fxPr9Y9AhLuz97KjjoZ7+WvadXweN
O69uOlSXw3Q2Bx6HUAJhGqcL0335M8x36EveFmmNIXe3kW+uO/1H2Z/7YShPJmCM
SlGBvK++LQTKQhhWQcZBfS7TJSRLoSsGuYOin6Icpt793IvIV9+UA4kFaMGyl88N
yNdMemhFKIxyFdVFMHtsvTaxjRmzRMo/LJ1ycZn1uu5eROPcj22+/06byjOmuqOr
LawXkSEl779TuEZW/+ykvQxdw5uOIKOhQ53+IEzdlYhjsWg0aCJgOB8Zq3ULv2VS
0UmZFX/clXhFNRACj1d+zZEyeA6mcpw0vu4vZLvNeQKBgQDaDHt9tStf9Ovh9ouq
3V+GA1Yarx9RDLzBkEEBKpE19dmY8UgVNV0cv2pyJOC4rNKNLw7JFad/ATwpfGMY
fk/Z7YpVI8OfgvjI98fwib2yg2VsJbhke02RTAnm/7mgamYbmF4T4GLrOOMoXqZW
VOiCAKQKHJ+sK92s5CJmDBfmSwKBgQDDhlhHi3n5vX4Rteijc9oaH7V7cZFXUOJG
FiMWutHZdXlxSIythQggIPbslxq7AfuINfIFgo6HOGfP/3D2XmXU4iY1Q9QjMy2b
phxjY0ZG+AL99v3O4u45DBZ8C16xi/bniRhJxzljYgLN5VXvO/YVmxib6+yWCA/P
TKzgq8oNpQKBgCqrkKsL/h38FwEUN0bLpXrbQklchdtdi76xVRc+VkZiAyAb74g+
9ia/CrylnNhm8ZkxYUpWk32WJ0jTD61mYof6JTz+D7Uycy8Y1iarPdUmQ33Db+8x
9f7+C14KIzBSQgMacSagnZr8ee+XfiOc4Bc4uuFDsreFqg7AYj7oFPE7AoGBAJbi
P5HGcnRk5LqqFEK+jlqGibgfJbep9VN8lctek74qR3NCNz1YYbLZfXOKD9isaPzu
FDxoSbDTuFjsmLGmmxKzCiUkmLopLlLk1xdjbsIpdbWiOq7CtG9Vgqxq4cJFbl2y
kAmvMfwdkGhvR+d78CUwMMdyQnps8jZYxzgBmcT5AoGBAM0W8PtJRjdGWB1iR0vw
Tjd1bZl8SBO+8EgOJtQ0r/tVNJHSy62xhsR8Mr+vFs4PnmmdMr57Nn3mhVUC9ZXb
1+kqDopsvxt+MwhCopjlqeyG7WOAc2HlexaxjxwYgleX4JHlQopIYY78jtID341t
U9iaTFrQkpnGzOdmLbbsmB7X
-----END PRIVATE KEY-----
"#;

#[test]
fn test_select_quic_real_transport_path() {
    let (server_addr, trusted_cert, server_handle) =
        spawn_quic_echo_server(DSOFTBUS_QUIC_DEFAULT_ALPN);
    let payload = b"quic-echo-proof";
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN],
        trusted_server_certs: &[trusted_cert],
        payload,
        timeout: PROBE_TIMEOUT,
    })
    .expect("trusted QUIC probe should succeed");
    let outcome = select_transport_with_host_quic(
        TransportMode::Quic,
        DSOFTBUS_QUIC_DEFAULT_ALPN,
        Ok(probe.clone()),
    )
    .expect("strict quic mode should select quic after real probe");

    assert_eq!(probe.negotiated_alpn(), DSOFTBUS_QUIC_DEFAULT_ALPN);
    assert_eq!(probe.echoed_payload(), payload);
    assert_eq!(outcome.transport(), TransportKind::Quic);
    assert_eq!(outcome.markers(), &[MARKER_TRANSPORT_SELECTED_QUIC]);

    server_handle.join().expect("server thread join");
}

#[test]
fn test_reject_quic_wrong_alpn_real_transport() {
    let (server_addr, trusted_cert, server_handle) = spawn_quic_echo_server("h3");
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN, "h3"],
        trusted_server_certs: &[trusted_cert],
        payload: b"wrong-alpn",
        timeout: PROBE_TIMEOUT,
    });
    let err = probe.expect_err("probe must reject wrong negotiated ALPN");
    assert!(matches!(err, HostQuicProbeError::WrongAlpn { .. }));
    let selection =
        select_transport_with_host_quic(TransportMode::Quic, DSOFTBUS_QUIC_DEFAULT_ALPN, Err(err))
            .expect_err("strict quic must fail-closed on wrong alpn");
    assert_eq!(selection, TransportSelectionError::RejectQuicWrongAlpn);

    server_handle.join().expect("server thread join");
}

#[test]
fn test_reject_quic_invalid_or_untrusted_cert_real_transport() {
    let server_addr = reserve_then_release_udp_addr();
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN],
        trusted_server_certs: &[],
        payload: b"untrusted-cert",
        timeout: PROBE_TIMEOUT,
    });
    let err = probe.expect_err("probe must reject untrusted cert");
    assert_eq!(err, HostQuicProbeError::InvalidOrUntrustedCert);
    let selection =
        select_transport_with_host_quic(TransportMode::Quic, DSOFTBUS_QUIC_DEFAULT_ALPN, Err(err))
            .expect_err("strict quic must fail-closed on cert rejection");
    assert_eq!(
        selection,
        TransportSelectionError::RejectQuicInvalidOrUntrustedCert
    );
}

#[test]
fn test_reject_quic_strict_mode_downgrade_when_probe_unavailable() {
    let unavailable_addr = reserve_then_release_udp_addr();
    let (dummy_cert, _) = generate_server_material();
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr: unavailable_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN],
        trusted_server_certs: &[dummy_cert],
        payload: b"strict-downgrade",
        timeout: Duration::from_millis(150),
    });
    let err = probe.expect_err("probe must fail when no server is available");
    assert!(matches!(
        err,
        HostQuicProbeError::Unavailable(_) | HostQuicProbeError::Timeout
    ));
    let selection =
        select_transport_with_host_quic(TransportMode::Quic, DSOFTBUS_QUIC_DEFAULT_ALPN, Err(err))
            .expect_err("strict quic mode must reject downgrade");
    assert_eq!(
        selection,
        TransportSelectionError::RejectQuicStrictModeDowngrade
    );
}

#[test]
fn test_auto_mode_fallback_marker_emitted_when_probe_unavailable() {
    let unavailable_addr = reserve_then_release_udp_addr();
    let (dummy_cert, _) = generate_server_material();
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr: unavailable_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN],
        trusted_server_certs: &[dummy_cert],
        payload: b"auto-fallback",
        timeout: Duration::from_millis(150),
    });
    let err = probe.expect_err("probe must fail when no server is available");
    let outcome =
        select_transport_with_host_quic(TransportMode::Auto, DSOFTBUS_QUIC_DEFAULT_ALPN, Err(err))
            .expect("auto mode should fallback when live probe is unavailable");
    assert_eq!(outcome.transport(), TransportKind::Tcp);
    assert_eq!(
        outcome.markers(),
        &[
            MARKER_QUIC_OS_DISABLED_FALLBACK_TCP,
            MARKER_TRANSPORT_SELECTED_TCP,
            MARKER_SELFTEST_QUIC_FALLBACK_OK,
        ],
    );
}

#[test]
fn test_quic_carries_mux_contract_smoke_payload() {
    let stream_id = StreamId::new(7).expect("stream id");
    let priority = PriorityClass::new(2).expect("priority");
    let stream_name = StreamName::new("samgr.rpc").expect("stream name");

    let mut sender_mux = MuxHostEndpoint::new_authenticated(0);
    sender_mux
        .open_stream(
            stream_id,
            priority,
            stream_name.clone(),
            WindowCredit::new(8 * 1024),
        )
        .expect("open stream");
    let send_outcome = sender_mux
        .send_data(stream_id, priority, 128)
        .expect("send data event");
    assert!(
        matches!(send_outcome, dsoftbus::SendBudgetOutcome::Sent { .. }),
        "mux data event must be accepted"
    );

    let outbound_events = sender_mux.drain_outbound();
    assert_eq!(outbound_events.len(), 2, "open + data expected");
    let payload = encode_mux_wire_events(&outbound_events);

    let (server_addr, trusted_cert, server_handle) =
        spawn_quic_echo_server(DSOFTBUS_QUIC_DEFAULT_ALPN);
    let probe = probe_and_echo_once(HostQuicProbeRequest {
        server_addr,
        server_name: "localhost",
        expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
        offered_alpns: &[DSOFTBUS_QUIC_DEFAULT_ALPN],
        trusted_server_certs: &[trusted_cert],
        payload: &payload,
        timeout: PROBE_TIMEOUT,
    })
    .expect("trusted QUIC probe should carry mux smoke payload");
    let echoed_events =
        decode_mux_wire_events(probe.echoed_payload()).expect("decode echoed mux wire events");

    let mut receiver_mux = MuxHostEndpoint::new_authenticated(0);
    for event in echoed_events {
        let _ = receiver_mux.ingest(event).expect("ingest mux wire event");
    }
    let accepted = receiver_mux
        .accept_stream()
        .expect("accepted stream after open");
    assert_eq!(accepted.stream_id, stream_id);
    assert_eq!(accepted.priority, priority);
    assert_eq!(accepted.name, stream_name);
    assert_eq!(receiver_mux.buffered_bytes(stream_id), Some(128));

    server_handle.join().expect("server thread join");
}

fn spawn_quic_echo_server(
    server_alpn: &str,
) -> (SocketAddr, CertificateDer<'static>, thread::JoinHandle<()>) {
    let (cert, private_key) = generate_server_material();
    let server_config = build_server_config(vec![cert.clone()], private_key, server_alpn)
        .expect("build server config");
    let (addr_tx, addr_rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_io()
            .enable_time()
            .build()
            .expect("create tokio runtime");
        runtime.block_on(async move {
            let endpoint = Endpoint::server(server_config, SocketAddr::from(([127, 0, 0, 1], 0)))
                .expect("bind QUIC server endpoint");
            let addr = endpoint.local_addr().expect("server local addr");
            addr_tx.send(addr).expect("publish server addr");

            if let Some(incoming) = endpoint.accept().await {
                if let Ok(connection) = incoming.await {
                    if let Ok((mut send, mut recv)) = connection.accept_bi().await {
                        if let Ok(payload) = recv.read_to_end(MAX_ECHO_BYTES).await {
                            let _ = send.write_all(&payload).await;
                            let _ = send.finish();
                            // Keep the connection alive briefly so the peer can drain the echoed bytes.
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
            }

            endpoint.wait_idle().await;
        });
    });

    let addr = addr_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("receive server address from thread");
    (addr, cert, handle)
}

fn generate_server_material() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let cert = CertificateDer::from_pem_slice(TEST_SERVER_CERT_PEM.as_bytes())
        .expect("parse test certificate");
    let private_key = PrivateKeyDer::from_pem_slice(TEST_SERVER_KEY_PEM.as_bytes())
        .expect("parse test private key");
    (cert, private_key)
}

fn reserve_then_release_udp_addr() -> SocketAddr {
    let socket = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .expect("reserve local udp socket");
    let addr = socket.local_addr().expect("reserved addr");
    drop(socket);
    addr
}

fn encode_mux_wire_events(events: &[MuxWireEvent]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(events.len() as u32).to_le_bytes());
    for event in events {
        match event {
            MuxWireEvent::Open {
                stream_id,
                priority,
                name,
            } => {
                out.push(1);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
                let name_bytes = name.as_str().as_bytes();
                out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                out.extend_from_slice(name_bytes);
            }
            MuxWireEvent::OpenAck {
                stream_id,
                priority,
            } => {
                out.push(2);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
            }
            MuxWireEvent::Data {
                stream_id,
                priority,
                payload_len,
            } => {
                out.push(3);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
                out.extend_from_slice(&(*payload_len as u32).to_le_bytes());
            }
            MuxWireEvent::WindowUpdate {
                stream_id,
                priority,
                delta,
            } => {
                out.push(4);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
                out.extend_from_slice(&delta.to_le_bytes());
            }
            MuxWireEvent::Rst {
                stream_id,
                priority,
            } => {
                out.push(5);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
            }
            MuxWireEvent::Close {
                stream_id,
                priority,
            } => {
                out.push(6);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
            }
            MuxWireEvent::Ping {
                stream_id,
                priority,
            } => {
                out.push(7);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
            }
            MuxWireEvent::Pong {
                stream_id,
                priority,
            } => {
                out.push(8);
                out.extend_from_slice(&stream_id.get().to_le_bytes());
                out.push(priority.get());
            }
        }
    }
    out
}

fn decode_mux_wire_events(bytes: &[u8]) -> Result<Vec<MuxWireEvent>, String> {
    let mut offset = 0usize;
    let event_count = read_u32_le(bytes, &mut offset)? as usize;
    let mut events = Vec::with_capacity(event_count);

    for _ in 0..event_count {
        let tag = read_u8(bytes, &mut offset)?;
        let stream_id_raw = read_u32_le(bytes, &mut offset)?;
        let priority_raw = read_u8(bytes, &mut offset)?;
        let stream_id =
            StreamId::new(stream_id_raw).ok_or_else(|| "invalid stream id".to_string())?;
        let priority =
            PriorityClass::new(priority_raw).ok_or_else(|| "invalid priority".to_string())?;
        let event = match tag {
            1 => {
                let name_len = read_u16_le(bytes, &mut offset)? as usize;
                let name = read_bytes(bytes, &mut offset, name_len)?;
                let name_str =
                    std::str::from_utf8(name).map_err(|err| format!("invalid utf8 name: {err}"))?;
                let stream_name = StreamName::new(name_str)
                    .map_err(|err| format!("invalid stream name: {err}"))?;
                MuxWireEvent::Open {
                    stream_id,
                    priority,
                    name: stream_name,
                }
            }
            2 => MuxWireEvent::OpenAck {
                stream_id,
                priority,
            },
            3 => {
                let payload_len = read_u32_le(bytes, &mut offset)? as usize;
                MuxWireEvent::Data {
                    stream_id,
                    priority,
                    payload_len,
                }
            }
            4 => {
                let delta = read_i64_le(bytes, &mut offset)?;
                MuxWireEvent::WindowUpdate {
                    stream_id,
                    priority,
                    delta,
                }
            }
            5 => MuxWireEvent::Rst {
                stream_id,
                priority,
            },
            6 => MuxWireEvent::Close {
                stream_id,
                priority,
            },
            7 => MuxWireEvent::Ping {
                stream_id,
                priority,
            },
            8 => MuxWireEvent::Pong {
                stream_id,
                priority,
            },
            _ => return Err(format!("unknown mux event tag: {tag}")),
        };
        events.push(event);
    }

    if offset != bytes.len() {
        return Err("trailing bytes in mux payload".to_string());
    }
    Ok(events)
}

fn read_u8(bytes: &[u8], offset: &mut usize) -> Result<u8, String> {
    let value = *bytes
        .get(*offset)
        .ok_or_else(|| "unexpected eof while reading u8".to_string())?;
    *offset += 1;
    Ok(value)
}

fn read_u16_le(bytes: &[u8], offset: &mut usize) -> Result<u16, String> {
    let chunk = read_bytes(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn read_u32_le(bytes: &[u8], offset: &mut usize) -> Result<u32, String> {
    let chunk = read_bytes(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn read_i64_le(bytes: &[u8], offset: &mut usize) -> Result<i64, String> {
    let chunk = read_bytes(bytes, offset, 8)?;
    Ok(i64::from_le_bytes([
        chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
    ]))
}

fn read_bytes<'a>(bytes: &'a [u8], offset: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| "offset overflow while reading bytes".to_string())?;
    let chunk = bytes
        .get(*offset..end)
        .ok_or_else(|| "unexpected eof while reading bytes".to_string())?;
    *offset = end;
    Ok(chunk)
}
