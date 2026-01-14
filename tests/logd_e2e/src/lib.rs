//! CONTEXT: logd end-to-end test harness library
//! INTENT: Observability integration testing with logd journal + crash reports
//! IDL (target): APPEND(level,scope,msg,fields), QUERY(since_nsec,max_count), STATS()
//! DEPS: logd (service integration)
//! READINESS: Host backend ready; loopback transport established
//! TESTS: Journal roundtrip, overflow behavior, crash reports, query pagination
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]
#![forbid(unsafe_code)]
