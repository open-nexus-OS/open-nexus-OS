use identity::Identity;

#[test]
fn sign_and_verify_via_json_roundtrip() {
    let identity = Identity::generate().expect("identity generation");
    let exported = identity.to_json().expect("serialize");
    let restored = Identity::from_json(&exported).expect("deserialize");

    let payload = b"integration";
    let signature = restored.sign(payload);
    assert!(Identity::verify_with_key(&restored.verifying_key(), payload, &signature));
}
