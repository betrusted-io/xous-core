#![cfg(feature = "alloc")]

use hex_literal::hex;
use pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use slh_dsa::{Sha2_128s, SigningKey, VerifyingKey};
use std::ops::Deref;

// Serialization of the SLH-DSA keys is still a draft
// The vectors used here are taken from the draft-ietf-lamps-x509-slhdsa
// https://github.com/lamps-wg/x509-slhdsa/commit/128c68b6b141e109e3e0ec8f3f47c832a4baaa30
#[test]
fn pkcs8_output() {
    let signing = SigningKey::<Sha2_128s>::try_from(&hex!("A2263BCA45860836523160049523D621677FAD90D51EB6067A327E0D1E64A5012B8109EC777CAA4E1F024CCFCF9497D99180509280F4256AF2B07AF80289B494")[..]).unwrap();

    let out = signing.to_pkcs8_pem(LineEnding::LF).unwrap();

    // https://github.com/lamps-wg/x509-slhdsa/blob/main/id-slh-dsa-sha2-128s.priv
    assert_eq!(
        out.deref(),
        r#"-----BEGIN PRIVATE KEY-----
MFICAQAwCwYJYIZIAWUDBAMUBECiJjvKRYYINlIxYASVI9YhZ3+tkNUetgZ6Mn4N
HmSlASuBCex3fKpOHwJMz8+Ul9mRgFCSgPQlavKwevgCibSU
-----END PRIVATE KEY-----
"#
    );

    let parsed = SigningKey::<Sha2_128s>::from_pkcs8_pem(out.deref()).unwrap();

    assert_eq!(parsed, signing);

    let public: VerifyingKey<Sha2_128s> = parsed.as_ref().clone();

    let out = public.to_public_key_pem(LineEnding::LF).unwrap();

    // https://github.com/lamps-wg/x509-slhdsa/blob/main/id-slh-dsa-sha2-128s.pub
    assert_eq!(
        out.deref(),
        r#"-----BEGIN PUBLIC KEY-----
MDAwCwYJYIZIAWUDBAMUAyEAK4EJ7Hd8qk4fAkzPz5SX2ZGAUJKA9CVq8rB6+AKJ
tJQ=
-----END PUBLIC KEY-----
"#
    );
}
