@0xf0f0c5c005530003;

# CONTEXT: TASK-0253 `inputd` subscribe/keymap contract seed for merged live input.
# OWNERS: @ui @runtime
# STATUS: Experimental
# API_STABILITY: Unstable

struct SubscribeRequest {
}

struct SetKeymapRequest {
  name @0 :Text;
}

struct GetKeymapResponse {
  name @0 :Text;
}

enum InputEventKind {
  pointerMove @0;
  pointerDown @1;
  keyboard @2;
  touchDown @3;
  touchMove @4;
  touchUp @5;
  imeShow @6;
  imeHide @7;
}

struct InputEvent {
  timestampNs @0 :UInt64;
  kind @1 :InputEventKind;
  x @2 :Int32;
  y @3 :Int32;
  keyCode @4 :UInt32;
  repeated @5 :Bool;
}

struct SubscribeResponse {
  stream @0 :List(InputEvent);
}
