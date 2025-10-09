@0xb74f4d7b6ce50c7a;

struct GetDeviceIdRequest {
}

struct GetDeviceIdResponse {
  deviceId @0 :Text;
}

struct SignRequest {
  payload @0 :Data;
}

struct SignResponse {
  ok @0 :Bool;
  signature @1 :Data;
}

struct VerifyRequest {
  payload @0 :Data;
  signature @1 :Data;
  verifyingKey @2 :Data;
}

struct VerifyResponse {
  valid @0 :Bool;
}
