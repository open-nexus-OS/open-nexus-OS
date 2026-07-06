@0xc8e2f4a61d9b7350;
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# QuerySpec v1 — the queryd wire contract (docs/dev/dsl/db-queries.md)
#
# Frame: [opcode u8][capnp request]; response: [capnp response].
# Opcodes: 1 CREATE_TABLE, 2 PUT, 3 DELETE, 4 QUERY (source/services/queryd).
#
# Namespaces are DERIVED SERVER-SIDE from the caller's bundle identity —
# nothing on the wire selects a namespace, so apps cannot reach each other's
# tables by construction. Execution is gated by `nexus.permission.QUERY`
# (abilitymgr, fail-closed).
#
# The contract is engine-agnostic: error codes, ordering (order column with
# primary-key tie-break) and opaque tokens are defined HERE, not by the
# engine behind the service.

struct QVal {
  union {
    boolVal @0 :Bool;
    intVal  @1 :Int64;
    fxVal   @2 :Int64;    # raw Q32.32
    strVal  @3 :Text;
  }
}

enum ColType { bool @0; int @1; fx @2; str @3; }

enum QueryOp { eq @0; ge @1; le @2; }

# Stable error vocabulary (mirrors the engine's QueryError + service gates).
enum QueryErr {
  unknownTable  @0;
  unknownColumn @1;
  typeMismatch  @2;
  unsupported   @3;
  badToken      @4;
  corrupt       @5;
  denied        @6;   # permission gate (fail-closed)
  badRequest    @7;   # malformed frame/request
}

struct CreateTableRequest {
  table   @0 :UInt16;
  names   @1 :List(Text);    # column names (the DSL speaks names)
  types   @2 :List(ColType);
  pkCol   @3 :UInt16;
  indexed @4 :List(UInt16);
}

struct PutRequest {
  table @0 :UInt16;
  row   @1 :List(QVal);
}

struct DeleteRequest {
  table @0 :UInt16;
  pk    @1 :QVal;
}

struct QueryPred {
  col   @0 :Text;            # column name
  op    @1 :QueryOp;
  value @2 :QVal;
}

struct QueryRequest {
  table      @0 :UInt16;
  preds      @1 :List(QueryPred);  # eq conjunction + ≤1 range on orderCol
  orderCol   @2 :Text;
  descending @3 :Bool;
  limit      @4 :UInt32;
  token      @5 :Data;             # empty = first page
}

struct Row { values @0 :List(QVal); }

struct QueryResponse {
  union {
    ok  @0 :PageResult;
    err @1 :QueryErr;
  }
}

struct PageResult {
  rows @0 :List(Row);
  next @1 :Data;                   # empty = exhausted
}

struct AckResponse {
  union {
    ok  @0 :Void;
    err @1 :QueryErr;
  }
}
