@0xa9a9a9a9a9a9a9a9;

struct GetAnchorsRequest {
}

struct GetAnchorsResponse {
  anchors @0 :List(Text);
}

struct VerifyRequest {
  anchorId @0 :Text;
  payload @1 :Data;
  signature @2 :Data;
}

struct VerifyResponse {
  ok @0 :Bool;
}

struct DeviceIdRequest {
}

struct DeviceIdResponse {
  id @0 :Text;
}

struct IsKeyAllowedRequest {
  publisher @0 :Text;
  alg @1 :Text;
  pubkey @2 :Data;
  # @3..@7 reserved-for-v2
}

struct IsKeyAllowedResponse {
  allowed @0 :Bool;
  reason @1 :Text;
  # @2..@5 reserved-for-v2
}
