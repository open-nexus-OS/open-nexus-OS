@0xfab90d8ab062b6c0;

struct Announce {
  deviceId @0 :Text;
  services @1 :List(Text);
  port @2 :UInt16;
}

struct ConnectRequest {
  deviceId @0 :Text;
}

struct ConnectResponse {
  ok @0 :Bool;
}

struct Frame {
  chan @0 :UInt32;
  bytes @1 :Data;
}
