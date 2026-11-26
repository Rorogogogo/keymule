use super::*;
use crate::store::testing::MemVault;
use p256::ecdsa::signature::Signer;

fn browser() -> (p256::ecdsa::SigningKey, Device) {
    use p256::elliptic_curve::Generate;
    let sk = p256::ecdsa::SigningKey::generate();
    let key = B64.encode(sk.verifying_key().to_sec1_point(false).as_bytes());
    (sk, new_device("Chrome on my Mac", &key).unwrap())
}

fn sign(sk: &p256::ecdsa::SigningKey, msg: &str) -> String {
    let sig: p256::ecdsa::Signature = sk.sign(msg.as_bytes());
    B64.encode(sig.to_bytes())
}

#[test]
fn only_a_paired_browser_can_answer() {
    let (sk, dev) = browser();
    let (other, _) = browser();
    let req = r#"{"id":"x"}"#;
    let mut reply = Reply {
        button: "Allow".into(),
        device: dev.id.clone(),
        sig: sign(&sk, &message("x", req, "Allow", None)),
        secret: None,
    };
    assert!(verify(std::slice::from_ref(&dev), "x", req, &reply).is_ok());

    reply.sig = sign(&other, &message("x", req, "Allow", None));
    assert!(
        verify(std::slice::from_ref(&dev), "x", req, &reply).is_err(),
        "another key"
    );

    reply.sig = sign(&sk, &message("x", req, "Cancel", None));
    assert!(
        verify(std::slice::from_ref(&dev), "x", req, &reply).is_err(),
        "another answer"
    );

    reply.sig = sign(&sk, &message("x", r#"{"id":"y"}"#, "Allow", None));
    assert!(
        verify(std::slice::from_ref(&dev), "x", req, &reply).is_err(),
        "another question"
    );

    reply.sig = sign(&sk, &message("x", req, "Allow", None));
    assert!(verify(&[], "x", req, &reply).is_err(), "not paired");
}

#[test]
fn a_typed_secret_only_opens_for_the_process_that_asked() {
    use aes_gcm::aead::{Aead, KeyInit};
    use p256::elliptic_curve::Generate;
    // The asking process's one-off key, and the page's side of ECDH.
    let asker = p256::SecretKey::generate();
    let page = p256::SecretKey::generate();
    let shared =
        p256::ecdh::diffie_hellman(page.to_nonzero_scalar(), asker.public_key().as_affine());
    let key = aes_key("id1", shared.raw_secret_bytes().as_slice());
    let iv = [7u8; 12];
    let ct = aes_gcm::Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(
            &aes_gcm::Nonce::try_from(&iv[..]).unwrap(),
            b"tok_123".as_ref(),
        )
        .unwrap();
    let sealed = SealedSecret {
        epk: B64.encode(page.public_key().to_sec1_bytes()),
        iv: B64.encode(iv),
        ct: B64.encode(ct),
    };
    assert_eq!(open_secret(&asker, "id1", &sealed).unwrap(), "tok_123");
    assert!(
        open_secret(&asker, "id2", &sealed).is_err(),
        "bound to the question"
    );
    let stranger = p256::SecretKey::generate();
    assert!(open_secret(&stranger, "id1", &sealed).is_err());
}

#[test]
fn a_shown_value_only_opens_in_the_browser_that_asked() {
    use aes_gcm::aead::{Aead, KeyInit};
    use p256::elliptic_curve::Generate;
    let keymule = p256::SecretKey::generate();
    let browser = p256::SecretKey::generate();
    let bpk = B64.encode(browser.public_key().to_sec1_bytes());
    let (iv, ct) = seal_for_browser(&keymule, "id1", &bpk, b"tok_123").unwrap();
    // What the page does: ECDH with keymule's one-off public key.
    let open = |sk: &p256::SecretKey, id: &str| {
        let shared =
            p256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), keymule.public_key().as_affine());
        let key = derive("keymule-show-v1", id, shared.raw_secret_bytes().as_slice());
        let iv = B64.decode(&iv).unwrap();
        aes_gcm::Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .decrypt(
                &aes_gcm::Nonce::try_from(iv.as_slice()).unwrap(),
                B64.decode(&ct).unwrap().as_slice(),
            )
            .ok()
    };
    assert_eq!(open(&browser, "id1").as_deref(), Some(&b"tok_123"[..]));
    assert!(open(&browser, "id2").is_none(), "bound to the question");
    assert!(
        open(&p256::SecretKey::generate(), "id1").is_none(),
        "only that browser"
    );
}

#[test]
fn only_a_paired_browser_can_log_in() {
    let (sk, dev) = browser();
    let sig = sign(&sk, &login_message("n1"));
    assert!(verify_login(std::slice::from_ref(&dev), &dev.id, "n1", &sig).is_ok());
    assert!(
        verify_login(std::slice::from_ref(&dev), &dev.id, "n2", &sig).is_err(),
        "signed for another challenge"
    );
    assert!(
        verify_login(&[], &dev.id, "n1", &sig).is_err(),
        "not paired"
    );
    let (other, _) = browser();
    let forged = sign(&other, &login_message("n1"));
    assert!(
        verify_login(std::slice::from_ref(&dev), &dev.id, "n1", &forged).is_err(),
        "another key"
    );
}

#[test]
fn devices_are_kept_in_the_vault() {
    let v = MemVault::default();
    let (_, dev) = browser();
    assert!(devices(&v).is_empty());
    add_device(&v, dev.clone()).unwrap();
    add_device(&v, dev.clone()).unwrap();
    assert_eq!(devices(&v), vec![dev.clone()], "pairing twice keeps one");
    assert!(remove_device(&v, &dev.id).unwrap());
    assert!(v.get(DEVICES).is_err());
    assert!(new_device("x", "not base64!").is_err());
}
