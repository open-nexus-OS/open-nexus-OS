// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Host-only QUIC probe backend for TASK-0021 selection/fallback contract
//! OWNERS: @runtime
//! STATUS: Functional (host QUIC probe + selection bridge)
//! API_STABILITY: Experimental
//! TEST_COVERAGE: `userspace/dsoftbus/tests/quic_host_transport_contract.rs`, `userspace/dsoftbus/tests/quic_selection_contract.rs`
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::{HandshakeData, QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, ConnectionError, Endpoint};
use rustls::crypto::ring::default_provider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::RootCertStore;
use thiserror::Error;

use crate::transport_selection::{
    select_transport, QuicProbe, TransportMode, TransportSelectionError, TransportSelectionOutcome,
};

pub const DSOFTBUS_QUIC_DEFAULT_ALPN: &str = "nexus.dsoftbus.v1";

#[derive(Debug)]
pub struct HostQuicProbeRequest<'a> {
    pub server_addr: SocketAddr,
    pub server_name: &'a str,
    pub expected_alpn: &'a str,
    pub offered_alpns: &'a [&'a str],
    pub trusted_server_certs: &'a [CertificateDer<'static>],
    pub payload: &'a [u8],
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "host QUIC probe results must be validated by transport selection"]
pub struct HostQuicProbeResult {
    negotiated_alpn: String,
    echoed_payload: Vec<u8>,
}

impl HostQuicProbeResult {
    pub fn negotiated_alpn(&self) -> &str {
        &self.negotiated_alpn
    }

    pub fn echoed_payload(&self) -> &[u8] {
        &self.echoed_payload
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[must_use = "host QUIC probe errors must be handled as explicit reject/fallback decisions"]
pub enum HostQuicProbeError {
    #[error("quic reject: wrong alpn ({negotiated_alpn})")]
    WrongAlpn { negotiated_alpn: String },
    #[error("quic reject: invalid or untrusted cert")]
    InvalidOrUntrustedCert,
    #[error("quic unavailable: {0}")]
    Unavailable(String),
    #[error("quic probe timed out")]
    Timeout,
    #[error("quic protocol error: {0}")]
    Protocol(String),
}

pub fn build_server_config(
    cert_chain: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
    alpn: &str,
) -> Result<quinn::ServerConfig, HostQuicProbeError> {
    let mut server_crypto = rustls::ServerConfig::builder_with_provider(default_provider().into())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|err| HostQuicProbeError::Protocol(format!("tls version config failed: {err}")))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|err| HostQuicProbeError::Protocol(format!("server cert config failed: {err}")))?;
    server_crypto.alpn_protocols = vec![alpn.as_bytes().to_vec()];

    let quic_crypto = QuicServerConfig::try_from(server_crypto)
        .map_err(|err| HostQuicProbeError::Protocol(format!("quic server config failed: {err}")))?;
    Ok(quinn::ServerConfig::with_crypto(Arc::new(quic_crypto)))
}

pub fn probe_and_echo_once(
    request: HostQuicProbeRequest<'_>,
) -> Result<HostQuicProbeResult, HostQuicProbeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| {
            HostQuicProbeError::Unavailable(format!("tokio runtime init failed: {err}"))
        })?;
    runtime.block_on(probe_and_echo_once_async(request))
}

async fn probe_and_echo_once_async(
    request: HostQuicProbeRequest<'_>,
) -> Result<HostQuicProbeResult, HostQuicProbeError> {
    if request.trusted_server_certs.is_empty() {
        return Err(HostQuicProbeError::InvalidOrUntrustedCert);
    }

    let client_config = build_client_config(
        request.expected_alpn,
        request.offered_alpns,
        request.trusted_server_certs,
    )?;
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let mut endpoint = Endpoint::client(bind_addr).map_err(|err| {
        HostQuicProbeError::Unavailable(format!("client endpoint bind failed: {err}"))
    })?;
    endpoint.set_default_client_config(client_config);

    let connecting = endpoint
        .connect(request.server_addr, request.server_name)
        .map_err(|err| {
            HostQuicProbeError::Unavailable(format!("client connect setup failed: {err}"))
        })?;
    let connection = match tokio::time::timeout(request.timeout, connecting).await {
        Ok(Ok(connection)) => connection,
        Ok(Err(err)) => {
            endpoint.wait_idle().await;
            return Err(classify_connect_failure(err));
        }
        Err(_) => {
            endpoint.wait_idle().await;
            return Err(HostQuicProbeError::Unavailable(
                "connect timed out waiting for QUIC handshake".to_string(),
            ));
        }
    };

    let negotiated_alpn = negotiated_alpn_string(&connection)?;
    if negotiated_alpn != request.expected_alpn {
        connection.close(0u32.into(), b"reject wrong alpn");
        endpoint.wait_idle().await;
        return Err(HostQuicProbeError::WrongAlpn { negotiated_alpn });
    }

    let (mut send, mut recv) =
        match tokio::time::timeout(request.timeout, connection.open_bi()).await {
            Ok(Ok(streams)) => streams,
            Ok(Err(err)) => {
                connection.close(0u32.into(), b"open_bi fail");
                endpoint.wait_idle().await;
                return Err(HostQuicProbeError::Unavailable(format!(
                    "open_bi failed: {err}"
                )));
            }
            Err(_) => {
                connection.close(0u32.into(), b"open_bi timeout");
                endpoint.wait_idle().await;
                return Err(HostQuicProbeError::Unavailable(
                    "open_bi timed out".to_string(),
                ));
            }
        };

    match tokio::time::timeout(request.timeout, send.write_all(request.payload)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            connection.close(0u32.into(), b"write fail");
            endpoint.wait_idle().await;
            return Err(HostQuicProbeError::Unavailable(format!(
                "stream write failed: {err}"
            )));
        }
        Err(_) => {
            connection.close(0u32.into(), b"write timeout");
            endpoint.wait_idle().await;
            return Err(HostQuicProbeError::Unavailable(
                "stream write timed out".to_string(),
            ));
        }
    }

    send.finish()
        .map_err(|err| HostQuicProbeError::Unavailable(format!("stream finish failed: {err}")))?;
    drop(send);

    let mut echoed_payload = Vec::with_capacity(request.payload.len());
    while echoed_payload.len() < request.payload.len() {
        let remaining = request.payload.len() - echoed_payload.len();
        match tokio::time::timeout(request.timeout, recv.read_chunk(remaining, true)).await {
            Ok(Ok(Some(chunk))) => echoed_payload.extend_from_slice(&chunk.bytes),
            Ok(Ok(None)) => break,
            Ok(Err(err)) => {
                connection.close(0u32.into(), b"read fail");
                endpoint.wait_idle().await;
                return Err(HostQuicProbeError::Unavailable(format!(
                    "stream read failed: {err}"
                )));
            }
            Err(_) => {
                connection.close(0u32.into(), b"read timeout");
                endpoint.wait_idle().await;
                return Err(HostQuicProbeError::Unavailable(
                    "stream read timed out".to_string(),
                ));
            }
        }
    }
    if echoed_payload != request.payload {
        connection.close(0u32.into(), b"echo mismatch");
        endpoint.wait_idle().await;
        return Err(HostQuicProbeError::Protocol(
            "echo payload mismatch in QUIC probe".to_string(),
        ));
    }

    connection.close(0u32.into(), b"probe done");
    endpoint.wait_idle().await;

    Ok(HostQuicProbeResult {
        negotiated_alpn,
        echoed_payload,
    })
}

pub fn select_transport_with_host_quic(
    mode: TransportMode,
    expected_alpn: &str,
    probe_result: Result<HostQuicProbeResult, HostQuicProbeError>,
) -> Result<TransportSelectionOutcome, TransportSelectionError> {
    match probe_result {
        Ok(report) => select_transport(
            mode,
            QuicProbe::Candidate {
                expected_alpn,
                offered_alpn: report.negotiated_alpn(),
                cert_trusted: true,
            },
        ),
        Err(HostQuicProbeError::WrongAlpn { negotiated_alpn }) => select_transport(
            mode,
            QuicProbe::Candidate {
                expected_alpn,
                offered_alpn: negotiated_alpn.as_str(),
                cert_trusted: true,
            },
        ),
        Err(HostQuicProbeError::InvalidOrUntrustedCert) => select_transport(
            mode,
            QuicProbe::Candidate {
                expected_alpn,
                offered_alpn: expected_alpn,
                cert_trusted: false,
            },
        ),
        Err(HostQuicProbeError::Unavailable(_))
        | Err(HostQuicProbeError::Timeout)
        | Err(HostQuicProbeError::Protocol(_)) => select_transport(mode, QuicProbe::Disabled),
    }
}

fn build_client_config(
    expected_alpn: &str,
    offered_alpns: &[&str],
    trusted_server_certs: &[CertificateDer<'static>],
) -> Result<ClientConfig, HostQuicProbeError> {
    let mut roots = RootCertStore::empty();
    for cert in trusted_server_certs {
        roots.add(cert.clone()).map_err(|err| {
            HostQuicProbeError::Protocol(format!("invalid trusted server cert: {err}"))
        })?;
    }

    let mut client_crypto = rustls::ClientConfig::builder_with_provider(default_provider().into())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|err| HostQuicProbeError::Protocol(format!("tls version config failed: {err}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let mut alpn_protocols: Vec<Vec<u8>> = offered_alpns
        .iter()
        .map(|alpn| alpn.as_bytes().to_vec())
        .collect();
    if alpn_protocols.is_empty() {
        alpn_protocols.push(expected_alpn.as_bytes().to_vec());
    }
    client_crypto.alpn_protocols = alpn_protocols;

    let quic_crypto = QuicClientConfig::try_from(client_crypto)
        .map_err(|err| HostQuicProbeError::Protocol(format!("quic client config failed: {err}")))?;
    Ok(ClientConfig::new(Arc::new(quic_crypto)))
}

fn negotiated_alpn_string(connection: &quinn::Connection) -> Result<String, HostQuicProbeError> {
    let handshake = connection
        .handshake_data()
        .ok_or_else(|| HostQuicProbeError::Protocol("missing handshake data".to_string()))?;
    let handshake = handshake
        .downcast::<HandshakeData>()
        .map_err(|_| HostQuicProbeError::Protocol("unexpected handshake data type".to_string()))?;
    let protocol_bytes = handshake
        .protocol
        .as_ref()
        .ok_or_else(|| HostQuicProbeError::Protocol("missing negotiated ALPN".to_string()))?;
    let protocol = std::str::from_utf8(protocol_bytes).map_err(|err| {
        HostQuicProbeError::Protocol(format!("negotiated ALPN is not utf8: {err}"))
    })?;
    Ok(protocol.to_string())
}

fn classify_connect_failure(err: ConnectionError) -> HostQuicProbeError {
    HostQuicProbeError::Unavailable(format!("quic handshake failed: {err}"))
}
