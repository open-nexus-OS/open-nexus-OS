# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: RFC-0092 Service Exposure Contract — host SSOT of an exposure
# intent (the same fields as `[[expose."<subject>"]]` in policies/*.toml).
# The OS wire form is the nexus-wire frame of `ingressd` (RFC-0092 §3);
# this schema serves `nx` tooling and host tests. TLS values other than
# `none` are rejected until the network track delivers termination.
@0xd4a1e0c7b3f29a15;

enum Proto {
  tcp @0;
  udp @1;
}

enum Tls {
  none @0;
  tls @1;
  mtls @2;
}

struct ExposeIntent {
  service @0 :Text;          # declared subject; must equal the kernel-attributed sender at runtime
  port @1 :UInt16;           # NIC-facing port (unique per proto)
  proto @2 :Proto;
  cidrAllow @3 :List(Text);  # 1..=8 IPv4 CIDRs; "0.0.0.0/0" must be explicit
  ratePerS @4 :UInt32;       # 1..=10000
  burst @5 :UInt32;          # 1..=1000
  tls @6 :Tls;               # contract slot (RFC-0092 §4)
  backend @7 :UInt16;        # loopback backend port
}

enum ExposeReason {
  none @0;
  policy @1;                 # no declared exposure / capability refused
  identity @2;               # sender is not the declared subject
  limit @3;                  # bounds exceeded
  tls @4;                    # termination slot not delivered
}

struct ExposeResponse {
  ok @0 :Bool;
  reason @1 :ExposeReason;
}
