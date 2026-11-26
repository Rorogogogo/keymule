
use super::*;

#[test]
fn pasted_keys_are_checked() {
    use crate::presets::Kind;
    let pem = "-----BEGIN PRIVATE KEY-----\r\nMIIE\r\n-----END PRIVATE KEY-----\n";
    assert_eq!(
        check_other_kind(Kind::PrivateKey, pem).unwrap(),
        "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----"
    );
    assert!(check_other_kind(Kind::PrivateKey, "ssh-ed25519 AAAA… public").is_err());
    let (id, key) = (
        crate::detect::fake::aws_id(),
        crate::detect::fake::aws_secret(),
    );
    let pair = format!(" {id} \n\n {key} ");
    assert_eq!(
        check_other_kind(Kind::AwsKeys, &pair).unwrap(),
        format!("{id}\n{key}")
    );
    assert!(check_other_kind(Kind::AwsKeys, "just-one-line").is_err());
}
