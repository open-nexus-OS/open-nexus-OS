@0xc1f8fd7aa82ba3d0;

struct CheckRequest {
  subject @0 :Text;
  requiredCaps @1 :List(Text);
}

struct CheckResponse {
  allowed @0 :Bool;
  missing @1 :List(Text);
}
