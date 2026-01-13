@0xf0f0f0f0f0f0f0f0;

# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Log daemon IDL schema (v1)
#
# This schema serves as:
# - Documentation for the logging API
# - Type-safe code generation for host/std backend
# - Future API evolution tracking
#
# NOTE: OS/os-lite backend uses versioned byte frames (authoritative for v1 QEMU proof).
#       This schema is kept in sync but is not the on-wire format for OS builds.

enum LogLevel {
  error @0;
  warn @1;
  info @2;
  debug @3;
  trace @4;
}

struct AppendRequest {
  level @0 :LogLevel;
  scope @1 :Text;        # max 64 bytes (service/component name)
  message @2 :Text;      # max 256 bytes
  fields @3 :Data;       # opaque structured fields (CBOR/JSON), max 512 bytes
}

struct AppendResponse {
  ok @0 :Bool;
  recordId @1 :UInt64;   # monotonic record ID (0 on error)
  dropped @2 :UInt64;    # total dropped records (overflow counter)
}

struct QueryRequest {
  sinceNsec @0 :UInt64;  # timestamp (nanoseconds since boot)
  maxCount @1 :UInt16;   # max records to return (bounded)
}

struct QueryResponse {
  records @0 :List(LogRecord);
  total @1 :UInt64;      # total records in journal (not just returned)
  dropped @2 :UInt64;    # total dropped records (overflow)
}

struct LogRecord {
  recordId @0 :UInt64;
  timestampNsec @1 :UInt64;
  level @2 :LogLevel;
  serviceId @3 :UInt64;  # kernel-provided sender_service_id (unforgeable)
  scope @4 :Text;
  message @5 :Text;
  fields @6 :Data;       # opaque structured fields
}

struct StatsRequest {
  # empty (just returns current stats)
}

struct StatsResponse {
  totalRecords @0 :UInt64;
  droppedRecords @1 :UInt64;
  capacityRecords @2 :UInt32;
  capacityBytes @3 :UInt32;
}

# Crash report envelope (v1 minimal)
struct CrashReport {
  serviceName @0 :Text;
  pid @1 :UInt32;
  exitCode @2 :Int32;
  timestampNsec @3 :UInt64;
  recentLogs @4 :List(LogRecord);  # last N records (bounded)
}
