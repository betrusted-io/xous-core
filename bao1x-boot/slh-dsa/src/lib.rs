#![no_std]
#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)] // Be pedantic by default
//#![allow(non_snake_case)] // Allow notation matching the spec
#![allow(clippy::module_name_repetitions)] // There are many types of signature and otherwise this gets confusing
#![allow(clippy::similar_names)] // TODO: Consider resolving these
#![allow(clippy::clone_on_copy)] // Be explicit about moving data
#![deny(missing_docs)] // Require all public interfaces to be documented
#![warn(unreachable_pub)] // Prevent unexpected interface changes

//! # Usage
//! This crate implements the Stateless Hash-based Digital Signature Algorithm (SLH-DSA) based on the finalized
//! standard by NIST in FIPS-205. SLH-DSA (based on the SPHINCS+ submission) is a signature algorithm designed
//! to be resistant to quantum computers.
//!
//! While the API exposed by SLH-DSA is the same as conventional signature schemes, it is important
//! to note that the signatures produced by the algorithm are much larger than classical schemes like `EdDSA`,
//! ranging from over 7KB for the smallest parameter set to nearly 50KB at the largest
//!
//! This crate currently allocates signatures and intermediate values on the stack, which may cause problems for
//! environments with limited stack space.
//!
//!
//! ```
//! use slh_dsa::*;
//! use signature::*;
//!
//! let mut rng = rand::rng();
//!
//! // Generate a signing key using the SHAKE128f parameter set
//! let sk = SigningKey::<Shake128f>::new(&mut rng);
//!
//! // Generate the corresponding public key
//! let vk = sk.verifying_key();
//!
//! // Serialize the verifying key and distribute
//! let vk_bytes = vk.to_bytes();
//!
//! // Sign a message
//! let message = b"Hello world";
//! let sig = sk.sign_with_rng(&mut rng, message); // .sign() can be used for deterministic signatures
//!
//! // Deserialize a verifying key
//! let vk_deserialized = vk_bytes.try_into().unwrap();
//! assert_eq!(vk, vk_deserialized);
//!
//! assert!(vk_deserialized.verify(message, &sig).is_ok())
//! ```

#[cfg(feature = "alloc")]
extern crate alloc;

pub use signature;

mod address;
mod fors;
mod hashes;
mod hypertree;
mod signature_encoding;
mod signing_key;
mod util;
mod verifying_key;
mod wots;
mod xmss;

pub use hashes::*;
pub use signature_encoding::*;
pub use signing_key::*;
pub use verifying_key::*;

use fors::ForsParams;

/// Specific parameters for each of the 12 FIPS parameter sets
#[allow(private_bounds)] // Intentionally un-usable type
pub trait ParameterSet: ForsParams + SigningKeyLen + VerifyingKeyLen + SignatureLen {
    /// Human-readable name for parameter set, matching the FIPS-205 designations
    const NAME: &'static str;

    /// Associated OID with the Parameter
    const ALGORITHM_OID: pkcs8::ObjectIdentifier;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;
    use signature::*;
    use util::macros::test_parameter_sets;

    fn test_sign_verify<P: ParameterSet>() {
        let mut rng = rand::rng();
        let sk = SigningKey::<P>::new(&mut rng);
        let vk = sk.verifying_key();
        let msg = b"Hello, world!";
        let sig = sk.try_sign(msg).unwrap();
        vk.verify(msg, &sig).unwrap();
    }
    test_parameter_sets!(test_sign_verify);

    // Check signature fails on modified message
    #[test]
    fn test_sign_verify_shake_128f_fail_on_modified_message() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let msg = b"Hello, world!";
        let modified_msg = b"Goodbye, world!";

        let sig = sk.try_sign(msg).unwrap();
        let vk = sk.verifying_key();
        assert!(vk.verify(msg, &sig).is_ok());
        assert!(vk.verify(modified_msg, &sig).is_err());
    }

    #[test]
    fn test_sign_verify_fail_with_wrong_verifying_key() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let wrong_sk = SigningKey::<Shake128f>::new(&mut rng); // Generate a different signing key
        let msg = b"Hello, world!";

        let sig = sk.try_sign(msg).unwrap();
        let vk = sk.verifying_key();
        let wrong_vk = wrong_sk.verifying_key(); // Get the verifying key of the wrong signing key
        assert!(vk.verify(msg, &sig).is_ok());
        assert!(wrong_vk.verify(msg, &sig).is_err()); // This should fail because the verifying key does not match the signing key used
    }

    #[test]
    fn test_sign_verify_fail_on_modified_signature() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let msg = b"Hello, world!";

        let mut sig_bytes = sk.try_sign(msg).unwrap().to_bytes();
        // Randomly modify one byte in the signature
        let sig_len = sig_bytes.len();
        let random_byte_index = rng.random_range(0..sig_len);
        sig_bytes[random_byte_index] ^= 0xff; // Invert one byte to ensure it's different
        let sig = (&sig_bytes).into();

        let vk = sk.verifying_key();
        assert!(
            vk.verify(msg, &sig).is_err(),
            "Verification should fail with a modified signature"
        );
    }

    #[test]
    fn test_successive_signatures_not_equal() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let msg = b"Hello, world!";

        let sig1 = sk.try_sign_with_rng(&mut rng, msg).unwrap();
        let sig2 = sk.try_sign_with_rng(&mut rng, msg).unwrap();

        assert_ne!(
            sig1, sig2,
            "Two successive randomized signatures over the same message should not be equal"
        );
    }

    #[test]
    fn test_sign_verify_nonempty_context() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let vk = sk.verifying_key();
        let msg = b"Hello, world!";
        let ctx = b"Test context";
        let sig = sk.try_sign_with_context(msg, ctx, None).unwrap();
        vk.try_verify_with_context(msg, ctx, &sig).unwrap();
    }

    #[test]
    fn test_sign_verify_wrong_context() {
        let mut rng = rand::rng();
        let sk = SigningKey::<Shake128f>::new(&mut rng);
        let vk = sk.verifying_key();
        let msg = b"Hello, world!";
        let ctx = b"Test context!";
        let wrong_ctx = b"Wrong context";
        let sig = sk.try_sign_with_context(msg, ctx, None).unwrap();
        assert!(vk.try_verify_with_context(msg, wrong_ctx, &sig).is_err());
    }
}
