// Copyright 2019 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod key;

#[cfg(feature = "with_ctap2_1")]
use crate::ctap::data_formats::{extract_array, extract_text_string};
use crate::ctap::data_formats::{CredentialProtectionPolicy, PublicKeyCredentialSource};
use crate::ctap::key_material;
use crate::ctap::pin_protocol_v1::PIN_AUTH_LENGTH;
use crate::ctap::status_code::Ctap2StatusCode;
use crate::ctap::INITIAL_SIGNATURE_COUNTER;
use std::io::{Write, Read, SeekFrom, Seek};
#[cfg(feature = "with_ctap2_1")]
use std::string::String;
use std::vec;
use std::vec::Vec;
use arrayref::array_ref;
#[cfg(feature = "with_ctap2_1")]
use cbor::cbor_array_vec;
use core::convert::TryInto;
use ctap_crypto::rng256::Rng256;
use pddb::Pddb;
use rand_core::{OsRng, RngCore};
use std::cell::RefCell;

use self::key::PIN_RETRIES;

use super::CREDENTIAL_ID_SIZE;
/// Size hint for storing a credential record. It can grow larger than this
/// because things like icons and free-form strings are supported for storing
/// credentials, but what happens in this case is just a re-allocation in the PDDB.
/// Whereas if this number is "too big" you end up with wasted space. I think a
/// typical record will be around 300-400 bytes, so, this is a good compromise.
pub(crate) const CRED_INITAL_SIZE: usize = 512;

// Those constants may be modified before compilation to tune the behavior of the key.
//
// The number of pages should be at least 3 and at most what the flash can hold. There should be no
// reason to put a small number here, except that the latency of flash operations is linear in the
// number of pages. This may improve in the future. Currently, using 20 pages gives between 20ms and
// 240ms per operation. The rule of thumb is between 1ms and 12ms per additional page.
//
// Limiting the number of residential keys permits to ensure a minimum number of counter increments.
// Let:
// - P the number of pages (NUM_PAGES)
// - K the maximum number of residential keys (MAX_SUPPORTED_RESIDENTIAL_KEYS)
// - S the maximum size of a residential key (about 500)
// - C the number of erase cycles (10000)
// - I the minimum number of counter increments
//
// We have: I = (P * 4084 - 5107 - K * S) / 8 * C
//
// With P=20 and K=150, we have I=2M which is enough for 500 increments per day for 10 years.
#[allow(dead_code)] // openSK legacy
const NUM_PAGES: usize = 20;

/// The limit is based on the performance of the PDDB to get a query response within
/// the compliance time-out limit. Note that most keys support about 25 credentials.
const MAX_SUPPORTED_RESIDENTIAL_KEYS: usize = 150;

const MAX_PIN_RETRIES: u8 = 8;
#[cfg(feature = "with_ctap2_1")]
const DEFAULT_MIN_PIN_LENGTH: u8 = 4;
// TODO(kaczmarczyck) use this for the minPinLength extension
// https://github.com/google/OpenSK/issues/129
#[cfg(feature = "with_ctap2_1")]
const _DEFAULT_MIN_PIN_LENGTH_RP_IDS: Vec<String> = Vec::new();
// TODO(kaczmarczyck) Check whether this constant is necessary, or replace it accordingly.
#[cfg(feature = "with_ctap2_1")]
const _MAX_RP_IDS_LENGTH: usize = 8;

const FIDO_DICT: &'static str = "fido.cfg";
use crate::ctap::FIDO_CRED_DICT;
const FIDO_PERSISTENT_DICT: &'static str = "fido.persistent";

/// Wrapper for master keys.
pub struct MasterKeys {
    /// Master encryption key.
    pub encryption: [u8; 32],

    /// Master hmac key.
    pub hmac: [u8; 32],
}

/// CTAP persistent storage.
pub struct PersistentStore {
    pddb: RefCell::<Pddb>,
}

use std::num::ParseIntError;

pub fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

impl PersistentStore {
    /// Gives access to the persistent store.
    ///
    /// # Safety
    ///
    /// This should be at most one instance of persistent store per program lifetime.
    pub fn new(_rng: &mut impl Rng256) -> PersistentStore {
        let mut store = PersistentStore {
            pddb: RefCell::new(Pddb::new()),
        };
        // block until the PDDB is mounted
        store.pddb.borrow().is_mounted_blocking();
        store.init().unwrap();
        store
    }

    /// Initializes the store by creating missing objects.
    fn init(&mut self) -> Result<(), Ctap2StatusCode> {

        // Generate and store the master keys if they are missing.
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::MASTER_KEYS,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
            Some(64), Some(crate::basis_change)
        ) {
            Ok(mut master_keys) => {
                let mk_attr = master_keys.attributes().unwrap(); // attribute fetches should not fail, so we don't kick it up. We want to see the panic at this line if it does fail.
                if mk_attr.len != 64 {
                    if mk_attr.len != 0 {
                        log::error!("Master key has an illegal length. Suspect PDDB corruption?");
                        return Err(Ctap2StatusCode::CTAP2_ERR_INVALID_CREDENTIAL);
                    }
                    let mut keys = [0u8; 64];
                    OsRng.fill_bytes(&mut keys);
                    master_keys.write(&keys)
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
            }
            _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }

        // Generate and store the CredRandom secrets if they are missing.
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::CRED_RANDOM_SECRET,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
            Some(64), Some(crate::basis_change)
        ) {
            Ok(mut cred_random) => {
                let cred_attr = cred_random.attributes().unwrap();
                if cred_attr.len != 64 {
                    if cred_attr.len != 0 {
                        log::error!("Random credentials has an illegal length. Suspect PDDB corruption?");
                        return Err(Ctap2StatusCode::CTAP2_ERR_INVALID_CREDENTIAL);
                    }
                    let mut creds = [0u8; 64];
                    OsRng.fill_bytes(&mut creds);
                    cred_random.write(&creds)
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
            }
            _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }

        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::AAGUID,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
            Some(16), Some(crate::basis_change)
        ) {
            Ok(mut aaguid) => {
                let aaguid_attr = aaguid.attributes().unwrap();
                if aaguid_attr.len != 16 {
                    if aaguid_attr.len != 0 {
                        log::error!("AAGUID has an illegal length. Suspect PDDB corruption?");
                        return Err(Ctap2StatusCode::CTAP2_ERR_INVALID_CREDENTIAL);
                    }
                    aaguid.write(key_material::AAGUID)
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
            }
            _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }

        /*
            The point of the attestation key is to delegate the question of the authenticity of your U2F
            key effectively to a transaction that happens between the factory and the server. If the
            attestation private key is an impenetrable secret and you can trust the factory, then the
            attestation key effectively proves the U2F token was made by the factory to its specifications.

            In the case of Precursor's threat model, you don't trust the factory, and you assume there is
            basically no such thing as an impenetrable secret. So an attestation key is equivalent to any
            private key created entirely within your own device; its use proves that you're you, or at
            least, someone has possession of your device and your unlock password.

            Thus, for Precursor, there is no delegation of the question of authenticity -- you
            choose to verify the device to the level you're satisfied with. If you're happy to
            just take it out of the box and use it without checking anything, that's your choice,
            but you can also check everything as much as you like.

            As a result, the "attestation key" serves no role for Precursor users, other than to
            identify them as Precursor users. A unique per-device key would furthermore allow
            a server to uniquely identify each Precursor user. Therefore, we commit the private key
            into this public repository so any and all Precursor users may share it. The more
            widely it is used, the harder it becomes to de-anonymize a Precursor users.

            This key was generated using the script in `tools/gen_key_materials.sh`, and then printed
            out by calling
            `./configure.py --private-key=crypto_data/opensk.key --certificate=crypto_data/opensk_cert.pem`
         */
        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_PRIVATE_KEY,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
            Some(32), Some(crate::basis_change)
        ) {
            Ok(mut aapriv) => {
                let aapriv_attr = aapriv.attributes().unwrap();
                if aapriv_attr.len != 32 {
                    if aapriv_attr.len != 0 {
                        log::error!("AAPRIV has an illegal length. Suspect PDDB corruption?");
                        return Err(Ctap2StatusCode::CTAP2_ERR_INVALID_CREDENTIAL);
                    }
                    let pk = decode_hex("b8c3abd05cbe17b2faf87659c6f73e8467832112a0e609807cb68996c9a0c6a8").unwrap();
                    log::info!("writing PK of length {}", pk.len());
                    log::debug!("pk: {:x?}", pk);
                    assert!(pk.len() == 32, "PK len is wrong");
                    aapriv.write(&pk)
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
            }
            _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }

        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_CERTIFICATE,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
            Some(512), Some(crate::basis_change)
        ) {
            Ok(mut cert) => {
                let cert_attr = cert.attributes().unwrap();
                if cert_attr.len == 0 {
                    // DER-encoded public key
                    let der = decode_hex(
                        "308201423081e9021449a3e2a4e1078eae2f1a18567f0a734b09db2478300a06\
                         082a8648ce3d040302301f311d301b06035504030c14507265637572736f7220\
                         55324620444959204341301e170d3232303630353138313233395a170d333230\
                         3630343138313233395a30293127302506035504030c1e507265637572736f72\
                         2053656c662d5665726966696564204465766963653059301306072a8648ce3d\
                         020106082a8648ce3d030107034200042d8fad76c25851ee70ae651ab2361bee\
                         1b5d93769aff99d95ad112871060f2f342fb79566fee9e1bade250a29cd65012\
                         55e731531755284bfcbe85ada1f39f44300a06082a8648ce3d04030203480030\
                         45022100d27e39187da5efaa376254329499bb705f7188dd8c78e0725c7ed28d\
                         e5d218e1022070853e1a43707298e07ebe9b9eb7cd5839b794db4c3d22209554\
                         1f0bdf82d1f4").unwrap();
                    log::info!("writing cert of length {}", der.len());
                    log::debug!("der: {:x?}", der);
                    cert.write(&der)
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
            }
            _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }
        self.pddb.borrow().sync()
        .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;

        Ok(())
    }
    /* Notes in case we want to muck with the above certificates more later on:
private key:
-----BEGIN EC PARAMETERS-----
BggqhkjOPQMBBw==
-----END EC PARAMETERS-----
-----BEGIN EC PRIVATE KEY-----
MHcCAQEEILjDq9Bcvhey+vh2Wcb3PoRngyESoOYJgHy2iZbJoMaooAoGCCqGSM49
AwEHoUQDQgAELY+tdsJYUe5wrmUasjYb7htdk3aa/5nZWtEShxBg8vNC+3lWb+6e
G63iUKKc1lASVecxUxdVKEv8voWtofOfRA==
-----END EC PRIVATE KEY-----

certificate:
-----BEGIN CERTIFICATE-----
MIIBQjCB6QIUSaPipOEHjq4vGhhWfwpzSwnbJHgwCgYIKoZIzj0EAwIwHzEdMBsG
A1UEAwwUUHJlY3Vyc29yIFUyRiBESVkgQ0EwHhcNMjIwNjA1MTgxMjM5WhcNMzIw
NjA0MTgxMjM5WjApMScwJQYDVQQDDB5QcmVjdXJzb3IgU2VsZi1WZXJpZmllZCBE
ZXZpY2UwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQtj612wlhR7nCuZRqyNhvu
G12Tdpr/mdla0RKHEGDy80L7eVZv7p4breJQopzWUBJV5zFTF1UoS/y+ha2h859E
MAoGCCqGSM49BAMCA0gAMEUCIQDSfjkYfaXvqjdiVDKUmbtwX3GI3Yx44HJcftKN
5dIY4QIgcIU+GkNwcpjgfr6bnrfNWDm3lNtMPSIglVQfC9+C0fQ=
-----END CERTIFICATE-----

analysis:
openssl x509 -in opensk_cert.pem -inform pem -noout -text
Certificate:
    Data:
        Version: 1 (0x0)
        Serial Number:
            49:a3:e2:a4:e1:07:8e:ae:2f:1a:18:56:7f:0a:73:4b:09:db:24:78
        Signature Algorithm: ecdsa-with-SHA256
        Issuer: CN = Precursor U2F DIY CA
        Validity
            Not Before: Jun  5 18:12:39 2022 GMT
            Not After : Jun  4 18:12:39 2032 GMT
        Subject: CN = Precursor Self-Verified Device
        Subject Public Key Info:
            Public Key Algorithm: id-ecPublicKey
                Public-Key: (256 bit)
                pub:
                    04:2d:8f:ad:76:c2:58:51:ee:70:ae:65:1a:b2:36:
                    1b:ee:1b:5d:93:76:9a:ff:99:d9:5a:d1:12:87:10:
                    60:f2:f3:42:fb:79:56:6f:ee:9e:1b:ad:e2:50:a2:
                    9c:d6:50:12:55:e7:31:53:17:55:28:4b:fc:be:85:
                    ad:a1:f3:9f:44
                ASN1 OID: prime256v1
                NIST CURVE: P-256
    Signature Algorithm: ecdsa-with-SHA256
         30:45:02:21:00:d2:7e:39:18:7d:a5:ef:aa:37:62:54:32:94:
         99:bb:70:5f:71:88:dd:8c:78:e0:72:5c:7e:d2:8d:e5:d2:18:
         e1:02:20:70:85:3e:1a:43:70:72:98:e0:7e:be:9b:9e:b7:cd:
         58:39:b7:94:db:4c:3d:22:20:95:54:1f:0b:df:82:d1:f4

openssl asn1parse -in opensk_cert.pem -inform pem
    0:d=0  hl=4 l= 322 cons: SEQUENCE
    4:d=1  hl=3 l= 233 cons: SEQUENCE
    7:d=2  hl=2 l=  20 prim: INTEGER           :49A3E2A4E1078EAE2F1A18567F0A734B09DB2478
   29:d=2  hl=2 l=  10 cons: SEQUENCE
   31:d=3  hl=2 l=   8 prim: OBJECT            :ecdsa-with-SHA256
   41:d=2  hl=2 l=  31 cons: SEQUENCE
   43:d=3  hl=2 l=  29 cons: SET
   45:d=4  hl=2 l=  27 cons: SEQUENCE
   47:d=5  hl=2 l=   3 prim: OBJECT            :commonName
   52:d=5  hl=2 l=  20 prim: UTF8STRING        :Precursor U2F DIY CA
   74:d=2  hl=2 l=  30 cons: SEQUENCE
   76:d=3  hl=2 l=  13 prim: UTCTIME           :220605181239Z
   91:d=3  hl=2 l=  13 prim: UTCTIME           :320604181239Z
  106:d=2  hl=2 l=  41 cons: SEQUENCE
  108:d=3  hl=2 l=  39 cons: SET
  110:d=4  hl=2 l=  37 cons: SEQUENCE
  112:d=5  hl=2 l=   3 prim: OBJECT            :commonName
  117:d=5  hl=2 l=  30 prim: UTF8STRING        :Precursor Self-Verified Device
  149:d=2  hl=2 l=  89 cons: SEQUENCE
  151:d=3  hl=2 l=  19 cons: SEQUENCE
  153:d=4  hl=2 l=   7 prim: OBJECT            :id-ecPublicKey
  162:d=4  hl=2 l=   8 prim: OBJECT            :prime256v1
  172:d=3  hl=2 l=  66 prim: BIT STRING
  240:d=1  hl=2 l=  10 cons: SEQUENCE
  242:d=2  hl=2 l=   8 prim: OBJECT            :ecdsa-with-SHA256
  252:d=1  hl=2 l=  72 prim: BIT STRING
*/

    /// The credential ID, as stored in OpenSK, is a 112-entry Vec<u8> that starts with
    /// a random 128-bit AES IV. This effectively takes the 128-bit AES IV and turns it into
    /// a hex string that is suitable for indexing into the PDDB. Collisions
    /// are very rare in a 128-bit space, but the "full" credential is still checked
    /// after the lookup.
    fn cid_to_str(&self, credential_id: &[u8]) -> String {
        let mut hex = String::new();
        // yes, I do know the "hex" crate exists but have you looked at its dependency tree??
        for &b in credential_id[..16].iter() {
            hex.push_str(&format!("{:x}", b));
        }
        hex
    }

    /// Returns the first matching credential.
    ///
    /// Returns `None` if no credentials are matched or if `check_cred_protect` is set and the first
    /// matched credential requires user verification.
    pub fn find_credential(
        &self,
        rp_id: &str,
        credential_id: &[u8],
        check_cred_protect: bool,
    ) -> Result<Option<PublicKeyCredentialSource>, Ctap2StatusCode> {
        let shortid = self.cid_to_str(credential_id);
        match self.pddb.borrow().get(
            FIDO_CRED_DICT,
            &shortid,
            None, false, false,
            Some(CREDENTIAL_ID_SIZE), Some(crate::basis_change)
        ) {
            Ok(mut cred) => {
                log::trace!("find {}", shortid);
                let mut data = Vec::<u8>::new();
                cred.read_to_end(&mut data).or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                match deserialize_credential(&data) {
                    Some(result) => {
                        if result.credential_id == credential_id && result.rp_id == rp_id {
                            let user_verification_required =
                                result.cred_protect_policy == Some(CredentialProtectionPolicy::UserVerificationRequired);
                            if check_cred_protect && user_verification_required {
                                Ok(None)
                            } else {
                                Ok(Some(result))
                            }
                        } else {
                            Ok(None)
                        }
                    }
                    None => {
                        log::warn!("Credential entry {} did not deserialize", shortid);
                        Err(Ctap2StatusCode::CTAP2_ERR_INVALID_CREDENTIAL)
                    }
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => {
                    log::error!("PDDB internal error in find_credential: {:?}", e);
                    Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }, // PDDB internal error
            }
        }
    }

    /// Stores or updates a credential.
    ///
    /// If a credential with the same RP id and user handle already exists, it is replaced.
    pub fn store_credential(
        &mut self,
        new_credential: PublicKeyCredentialSource,
    ) -> Result<(), Ctap2StatusCode> {
        if self.count_credentials()? >= MAX_SUPPORTED_RESIDENTIAL_KEYS {
            return Err(Ctap2StatusCode::CTAP2_ERR_KEY_STORE_FULL);
        }
        let shortid = self.cid_to_str(&new_credential.credential_id);
        match self.pddb.borrow().get(
            FIDO_CRED_DICT,
            &shortid,
            None, true, true,
            Some(CRED_INITAL_SIZE), Some(crate::basis_change)
        ) {
            Ok(mut cred) => {
                let value = serialize_credential(new_credential)?;
                log::trace!("writing {}", shortid);
                cred.write(&value)
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                self.pddb.borrow().sync()
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                Ok(())
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the list of matching credentials.
    ///
    /// Does not return credentials that are not discoverable if `check_cred_protect` is set.
    pub fn filter_credential(
        &self,
        rp_id: &str,
        check_cred_protect: bool,
    ) -> Result<Vec<PublicKeyCredentialSource>, Ctap2StatusCode> {
        let mut result = Vec::<PublicKeyCredentialSource>::new();
        let cred_list = self.pddb.borrow().list_keys(
            FIDO_CRED_DICT, None).unwrap_or(Vec::new());
        for cred_name in cred_list.iter() {
            if let Some(mut cred_entry) = self.pddb.borrow().get(
                FIDO_CRED_DICT,
                cred_name,
                None, false, false,
                Some(CREDENTIAL_ID_SIZE), Some(crate::basis_change)
            ).ok() {
                log::trace!("checking {}", cred_name);
                let mut data = Vec::<u8>::new();
                cred_entry.read_to_end(&mut data).or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                if let Some(cred) = deserialize_credential(&data) {
                    if cred.rp_id == rp_id
                    && (cred.is_discoverable() || !check_cred_protect) {
                        log::trace!("filtered {}", cred_name);
                        result.push(cred);
                    }
                }
            }
        }
        Ok(result)
    }

    /// Returns the number of credentials.
    pub fn count_credentials(&self) -> Result<usize, Ctap2StatusCode> {
        let cred_list = self.pddb.borrow().list_keys(
            FIDO_CRED_DICT, None).unwrap_or(Vec::new());
        Ok(cred_list.len())
    }

    /// Returns the next creation order.
    pub fn new_creation_order(&self) -> Result<u64, Ctap2StatusCode> {
        let mut max = 0;
        let cred_list = self.pddb.borrow().list_keys(
            FIDO_CRED_DICT, None).unwrap_or(Vec::new());
        if cred_list.len() == 0 {
            return Ok(0)
        }
        for cred_name in cred_list.iter() {
            if let Some(mut cred_entry) = self.pddb.borrow().get(
                FIDO_CRED_DICT,
                cred_name,
                None, false, false,
                Some(CREDENTIAL_ID_SIZE), Some(crate::basis_change)
            ).ok() {
                let mut data = Vec::<u8>::new();
                cred_entry.read_to_end(&mut data).or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                if let Some(cred) = deserialize_credential(&data) {
                    max = max.max(cred.creation_order)
                }
            }
        }
        Ok(max.wrapping_add(1))
    }

    /// Returns the global signature counter.
    pub fn global_signature_counter(&self) -> Result<u32, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::GLOBAL_SIGNATURE_COUNTER,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(4), Some(crate::basis_change)
        ) {
            Ok(mut gsc) => {
                let mut value = [0u8; 4];
                match gsc.read(&mut value) {
                    Ok(4) => Ok(u32::from_ne_bytes(value)),
                    Ok(_) => {
                        gsc.seek(SeekFrom::Start(0)).ok(); // make sure we're writing to the beginning position
                        gsc.write(&INITIAL_SIGNATURE_COUNTER.to_ne_bytes())
                        .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                        self.pddb.borrow().sync()
                        .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                        Ok(INITIAL_SIGNATURE_COUNTER)
                    },
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
                }
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }
    }

    /// Increments the global signature counter.
    pub fn incr_global_signature_counter(&mut self, increment: u32) -> Result<(), Ctap2StatusCode> {
        let old_value = self.global_signature_counter()?;
        // In hopes that servers handle the wrapping gracefully.
        let new_value = old_value.wrapping_add(increment);
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::GLOBAL_SIGNATURE_COUNTER,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(4), Some(crate::basis_change)
        ) {
            Ok(mut gsc) => {
                gsc.write(&new_value.to_ne_bytes())
                .map(|_|())
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                self.pddb.borrow().sync()
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }
    }

    /// Returns the master keys.
    pub fn master_keys(&self) -> Result<MasterKeys, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::MASTER_KEYS,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false, None, Some(crate::basis_change)
        ) {
            Ok(mut mk) => {
                let mut master_keys = [0u8; 64];
                match mk.read(&mut master_keys) {
                    Ok(64) => {
                        Ok(MasterKeys {
                            encryption: *array_ref![master_keys, 0, 32],
                            hmac: *array_ref![master_keys, 32, 32],
                        })
                    }
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the CredRandom secret.
    pub fn cred_random_secret(&self, has_uv: bool) -> Result<[u8; 32], Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::CRED_RANDOM_SECRET,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false, None, Some(crate::basis_change)
        ) {
            Ok(mut crs) => {
                let mut cred_random_secret = [0u8; 64];
                match crs.read(&mut cred_random_secret) {
                    Ok(64) => {
                        let offset = if has_uv { 32 } else { 0 };
                        Ok(*array_ref![cred_random_secret, offset, 32])
                    }
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the PIN hash if defined.
    pub fn pin_hash(&self) -> Result<Option<[u8; PIN_AUTH_LENGTH]>, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::PIN_HASH,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false, None, Some(crate::basis_change)
        ) {
            Ok(mut ph) => {
                let mut pin_hash = [0u8; PIN_AUTH_LENGTH];
                match ph.read(&mut pin_hash) {
                    Ok(PIN_AUTH_LENGTH) => {
                        log::trace!("pin_hash: {:x?}", &pin_hash);
                        Ok(Some(pin_hash))
                    },
                    Ok(l) => {
                        log::error!("pin_hash read length incorrect: {}", l);
                        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                    }
                    Err(e) => {
                        log::error!("pin_hash returned an error: {:?}", e);
                        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                    },
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    log::info!("pin_hash not found");
                    Ok(None)
                },
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
        }
    }

    /// Sets the PIN hash.
    ///
    /// If it was already defined, it is updated.
    pub fn set_pin_hash(
        &mut self,
        pin_hash: &[u8; PIN_AUTH_LENGTH],
    ) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::PIN_HASH,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(PIN_AUTH_LENGTH), Some(crate::basis_change)
        ) {
            Ok(mut ph) => {
                match ph.write(pin_hash) {
                    Ok(PIN_AUTH_LENGTH) => {
                        self.pddb.borrow().sync()
                        .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                        Ok(())
                    },
                    Ok(l) => {
                        log::error!("set_pin_hash incorrect length: {}", l);
                        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                    }
                    Err(e) => {
                        log::error!("set_pin_hash write error: {:?}", e);
                        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                    }
                }
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the number of remaining PIN retries.
    pub fn pin_retries(&self) -> Result<u8, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::PIN_RETRIES,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false, None, Some(crate::basis_change)
        ) {
            Ok(mut pr) => {
                let mut value = [0u8; 1];
                match pr.read(&mut value) {
                    Ok(1) => Ok(value[0]),
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(MAX_PIN_RETRIES),
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
        }
    }

    /// Decrements the number of remaining PIN retries.
    pub fn decr_pin_retries(&mut self) -> Result<(), Ctap2StatusCode> {
        let old_value = self.pin_retries()?;
        let new_value = old_value.saturating_sub(1);
        if new_value != old_value {
            match self.pddb.borrow().get(
                FIDO_DICT,
                key::PIN_RETRIES,
                Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
                Some(1), Some(crate::basis_change)
            ) {
                Ok(mut pr) => {
                    pr.write(&[new_value])
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                    self.pddb.borrow().sync()
                    .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                }
                _ => return Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
            }
        }
        Ok(())
    }

    /// Resets the number of remaining PIN retries.
    pub fn reset_pin_retries(&mut self) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().delete_key(
            FIDO_DICT,
            PIN_RETRIES,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS)
        ) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
        }
    }

    /// Returns the minimum PIN length.
    #[cfg(feature = "with_ctap2_1")]
    pub fn min_pin_length(&self) -> Result<u8, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::MIN_PIN_LENGTH,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            false, false, None, Some(crate::basis_change)
        ) {
            Ok(mut pr) => {
                let mut value = [0u8; 1];
                match pr.read(&mut value) {
                    Ok(1) => Ok(value[0]),
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(DEFAULT_MIN_PIN_LENGTH),
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
        }
    }

    /// Sets the minimum PIN length.
    #[cfg(feature = "with_ctap2_1")]
    pub fn set_min_pin_length(&mut self, min_pin_length: u8) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::MIN_PIN_LENGTH,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(1), Some(crate::basis_change)
        ) {
            Ok(mut pl) => {
                match pl.write(&[min_pin_length]) {
                    Ok(1) => Ok(()),
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
                }
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }
    }

    /// Returns the list of RP IDs that are used to check if reading the minimum PIN length is
    /// allowed.
    #[cfg(feature = "with_ctap2_1")]
    pub fn _min_pin_length_rp_ids(&self) -> Result<Vec<String>, Ctap2StatusCode> {
        if let Some(mut mplri) = self.pddb.borrow().get(
            FIDO_DICT,
            key::_MIN_PIN_LENGTH_RP_IDS,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false,
            None, Some(crate::basis_change)
        ).ok() {
            let mut data = Vec::<u8>::new();
            mplri.read_to_end(&mut data).or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
            if let Some(list) = _deserialize_min_pin_length_rp_ids(&data) {
                Ok(list)
            } else {
                Ok(vec![])
            }
        } else {
            Ok(vec![])
        }
    }

    /// Sets the list of RP IDs that are used to check if reading the minimum PIN length is allowed.
    #[cfg(feature = "with_ctap2_1")]
    pub fn _set_min_pin_length_rp_ids(
        &mut self,
        min_pin_length_rp_ids: Vec<String>,
    ) -> Result<(), Ctap2StatusCode> {
        let mut min_pin_length_rp_ids = min_pin_length_rp_ids;
        for rp_id in _DEFAULT_MIN_PIN_LENGTH_RP_IDS {
            if !min_pin_length_rp_ids.contains(&rp_id) {
                min_pin_length_rp_ids.push(rp_id);
            }
        }
        if min_pin_length_rp_ids.len() > _MAX_RP_IDS_LENGTH {
            return Err(Ctap2StatusCode::CTAP2_ERR_KEY_STORE_FULL);
        }
        match self.pddb.borrow().get(
            FIDO_DICT,
            key::_MIN_PIN_LENGTH_RP_IDS,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(_MAX_RP_IDS_LENGTH), Some(crate::basis_change)
        ) {
            Ok(mut mrpli) => {
                mrpli.write(&_serialize_min_pin_length_rp_ids(min_pin_length_rp_ids)?)
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)).map(|_|())
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    // ---------------- persistent records ----------------------
    /// Returns the attestation private key if defined.
    pub fn attestation_private_key(
        &self,
    ) -> Result<Option<[u8; key_material::ATTESTATION_PRIVATE_KEY_LENGTH]>, Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_PRIVATE_KEY,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false,
            None, Some(crate::basis_change)
        ) {
            Ok(mut apk) => {
                let mut key = [0u8; key_material::ATTESTATION_PRIVATE_KEY_LENGTH];
                match apk.read(&mut key) {
                    Ok(key_material::ATTESTATION_PRIVATE_KEY_LENGTH) => {
                        Ok(Some(*array_ref![
                            key,
                            0,
                            key_material::ATTESTATION_PRIVATE_KEY_LENGTH
                        ]))
                    }
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
        }
    }

    /// Sets the attestation private key.
    ///
    /// If it is already defined, it is overwritten.
    pub fn set_attestation_private_key(
        &mut self,
        attestation_private_key: &[u8; key_material::ATTESTATION_PRIVATE_KEY_LENGTH],
    ) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_PRIVATE_KEY,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(key_material::ATTESTATION_PRIVATE_KEY_LENGTH), Some(crate::basis_change)
        ) {
            Ok(mut apk) => {
                apk.write(attestation_private_key)
                .map(|_|())
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                self.pddb.borrow().sync()
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the attestation certificate if defined.
    pub fn attestation_certificate(&self) -> Result<Option<Vec<u8>>, Ctap2StatusCode> {
        if let Some(mut acert) = self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_CERTIFICATE,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false,
            None, Some(crate::basis_change)
        ).ok() {
            let mut data = Vec::<u8>::new();
            match acert.read_to_end(&mut data) {
                Ok(l) => log::trace!("read {} bytes", l),
                Err(e) => log::error!("error reading certificate: {:?}", e),
            }
            log::trace!("read back cert: {:x?}", data);
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    /// Sets the attestation certificate.
    ///
    /// If it is already defined, it is overwritten.
    pub fn set_attestation_certificate(
        &mut self,
        attestation_certificate: &[u8],
    ) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::ATTESTATION_CERTIFICATE,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(1024), Some(crate::basis_change)
        ) {
            Ok(mut acert) => {
                acert.write(attestation_certificate)
                .map(|_|())
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                self.pddb.borrow().sync()
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
        }
    }

    /// Returns the AAGUID.
    pub fn aaguid(&self) -> Result<[u8; key_material::AAGUID_LENGTH], Ctap2StatusCode> {
        if let Some(mut guid) = self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::AAGUID,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, false,
            None, Some(crate::basis_change)
        ).ok() {
            let mut data = [0u8; key_material::AAGUID_LENGTH];
            match guid.read(&mut data) {
                Ok(key_material::AAGUID_LENGTH) => {
                    Ok(data)
                }
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR)
            }
        } else {
            Ok([0u8; key_material::AAGUID_LENGTH])
        }
    }

    /// Sets the AAGUID.
    ///
    /// If it is already defined, it is overwritten.
    #[allow(dead_code)]
    pub fn set_aaguid(
        &mut self,
        aaguid: &[u8; key_material::AAGUID_LENGTH],
    ) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().get(
            FIDO_PERSISTENT_DICT,
            key::AAGUID,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), false, true,
            Some(key_material::AAGUID_LENGTH), Some(crate::basis_change)
        ) {
            Ok(mut guid) => {
                guid.write(aaguid)
                .map(|_|())
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))?;
                self.pddb.borrow().sync()
                .or(Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR))
            }
            _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR),
        }
    }

    /// Resets the store as for a CTAP reset.
    ///
    /// In particular persistent entries are not reset.
    pub fn reset(&mut self, _rng: &mut impl Rng256) -> Result<(), Ctap2StatusCode> {
        match self.pddb.borrow().delete_dict(FIDO_DICT, Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS)) {
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
            }
            Ok(_) => Ok(()),
        }?;
        for basis in self.pddb.borrow().list_basis().iter () {
            match self.pddb.borrow().delete_dict(FIDO_CRED_DICT, Some(basis)) {
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
                }
                Ok(_) => Ok(()),
            }?;
            match self.pddb.borrow().delete_dict(crate::fido::U2F_APP_DICT, Some(basis)) {
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_INTERNAL_ERROR), // PDDB internal error
                }
                Ok(_) => Ok(()),
            }?;
        }
        // don't delete the persistent dictionary...
        self.init()?;
        Ok(())
    }
}

/// Deserializes a credential from storage representation.
pub(crate) fn deserialize_credential(data: &[u8]) -> Option<PublicKeyCredentialSource> {
    let cbor = cbor::read(data).ok()?;
    cbor.try_into().ok()
}

/// Serializes a credential to storage representation.
pub(crate) fn serialize_credential(credential: PublicKeyCredentialSource) -> Result<Vec<u8>, Ctap2StatusCode> {
    let mut data = Vec::new();
    if cbor::write(credential.into(), &mut data) {
        Ok(data)
    } else {
        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_RESPONSE_CANNOT_WRITE_CBOR)
    }
}

/// Deserializes a list of RP IDs from storage representation.
#[cfg(feature = "with_ctap2_1")]
fn _deserialize_min_pin_length_rp_ids(data: &[u8]) -> Option<Vec<String>> {
    let cbor = cbor::read(data).ok()?;
    extract_array(cbor)
        .ok()?
        .into_iter()
        .map(extract_text_string)
        .collect::<Result<Vec<String>, Ctap2StatusCode>>()
        .ok()
}

/// Serializes a list of RP IDs to storage representation.
#[cfg(feature = "with_ctap2_1")]
fn _serialize_min_pin_length_rp_ids(rp_ids: Vec<String>) -> Result<Vec<u8>, Ctap2StatusCode> {
    let mut data = Vec::new();
    if cbor::write(cbor_array_vec!(rp_ids), &mut data) {
        Ok(data)
    } else {
        Err(Ctap2StatusCode::CTAP2_ERR_VENDOR_RESPONSE_CANNOT_WRITE_CBOR)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::ctap::data_formats::{PublicKeyCredentialSource, PublicKeyCredentialType};
    use ctap_crypto::rng256::{Rng256, ThreadRng256};

    fn create_credential_source(
        rng: &mut ThreadRng256,
        rp_id: &str,
        user_handle: Vec<u8>,
    ) -> PublicKeyCredentialSource {
        let private_key = ctap_crypto::ecdsa::SecKey::gensk(rng);
        PublicKeyCredentialSource {
            key_type: PublicKeyCredentialType::PublicKey,
            credential_id: rng.gen_uniform_u8x32().to_vec(),
            private_key,
            rp_id: String::from(rp_id),
            user_handle,
            user_display_name: None,
            cred_protect_policy: None,
            creation_order: 0,
            user_name: None,
            user_icon: None,
        }
    }

    #[test]
    fn test_store() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        let credential_source = create_credential_source(&mut rng, "example.com", vec![]);
        assert!(persistent_store.store_credential(credential_source).is_ok());
        assert!(persistent_store.count_credentials().unwrap() > 0);
    }

    #[test]
    fn test_credential_order() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        let credential_source = create_credential_source(&mut rng, "example.com", vec![]);
        let current_latest_creation = credential_source.creation_order;
        assert!(persistent_store.store_credential(credential_source).is_ok());
        let mut credential_source = create_credential_source(&mut rng, "example.com", vec![]);
        credential_source.creation_order = persistent_store.new_creation_order().unwrap();
        assert!(credential_source.creation_order > current_latest_creation);
        let current_latest_creation = credential_source.creation_order;
        assert!(persistent_store.store_credential(credential_source).is_ok());
        assert!(persistent_store.new_creation_order().unwrap() > current_latest_creation);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_fill_store() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);

        // To make this test work for bigger storages, implement better int -> Vec conversion.
        assert!(MAX_SUPPORTED_RESIDENTIAL_KEYS < 256);
        for i in 0..MAX_SUPPORTED_RESIDENTIAL_KEYS {
            let credential_source =
                create_credential_source(&mut rng, "example.com", vec![i as u8]);
            assert!(persistent_store.store_credential(credential_source).is_ok());
            assert_eq!(persistent_store.count_credentials().unwrap(), i + 1);
        }
        let credential_source = create_credential_source(
            &mut rng,
            "example.com",
            vec![MAX_SUPPORTED_RESIDENTIAL_KEYS as u8],
        );
        assert_eq!(
            persistent_store.store_credential(credential_source),
            Err(Ctap2StatusCode::CTAP2_ERR_KEY_STORE_FULL)
        );
        assert_eq!(
            persistent_store.count_credentials().unwrap(),
            MAX_SUPPORTED_RESIDENTIAL_KEYS
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_overwrite() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        // These should have different IDs.
        let credential_source0 = create_credential_source(&mut rng, "example.com", vec![0x00]);
        let credential_source1 = create_credential_source(&mut rng, "example.com", vec![0x00]);
        let expected_credential = credential_source1.clone();

        assert!(persistent_store
            .store_credential(credential_source0)
            .is_ok());
        assert!(persistent_store
            .store_credential(credential_source1)
            .is_ok());
        assert_eq!(persistent_store.count_credentials().unwrap(), 1);
        assert_eq!(
            &persistent_store
                .filter_credential("example.com", false)
                .unwrap(),
            &[expected_credential]
        );

        // To make this test work for bigger storages, implement better int -> Vec conversion.
        assert!(MAX_SUPPORTED_RESIDENTIAL_KEYS < 256);
        for i in 0..MAX_SUPPORTED_RESIDENTIAL_KEYS {
            let credential_source =
                create_credential_source(&mut rng, "example.com", vec![i as u8]);
            assert!(persistent_store.store_credential(credential_source).is_ok());
            assert_eq!(persistent_store.count_credentials().unwrap(), i + 1);
        }
        let credential_source = create_credential_source(
            &mut rng,
            "example.com",
            vec![MAX_SUPPORTED_RESIDENTIAL_KEYS as u8],
        );
        assert_eq!(
            persistent_store.store_credential(credential_source),
            Err(Ctap2StatusCode::CTAP2_ERR_KEY_STORE_FULL)
        );
        assert_eq!(
            persistent_store.count_credentials().unwrap(),
            MAX_SUPPORTED_RESIDENTIAL_KEYS
        );
    }

    #[test]
    fn test_filter() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        let credential_source0 = create_credential_source(&mut rng, "example.com", vec![0x00]);
        let credential_source1 = create_credential_source(&mut rng, "example.com", vec![0x01]);
        let credential_source2 =
            create_credential_source(&mut rng, "another.example.com", vec![0x02]);
        let id0 = credential_source0.credential_id.clone();
        let id1 = credential_source1.credential_id.clone();
        assert!(persistent_store
            .store_credential(credential_source0)
            .is_ok());
        assert!(persistent_store
            .store_credential(credential_source1)
            .is_ok());
        assert!(persistent_store
            .store_credential(credential_source2)
            .is_ok());

        let filtered_credentials = persistent_store
            .filter_credential("example.com", false)
            .unwrap();
        assert_eq!(filtered_credentials.len(), 2);
        assert!(
            (filtered_credentials[0].credential_id == id0
                && filtered_credentials[1].credential_id == id1)
                || (filtered_credentials[1].credential_id == id0
                    && filtered_credentials[0].credential_id == id1)
        );
    }

    #[test]
    fn test_filter_with_cred_protect() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        let private_key = ctap_crypto::ecdsa::SecKey::gensk(&mut rng);
        let credential = PublicKeyCredentialSource {
            key_type: PublicKeyCredentialType::PublicKey,
            credential_id: rng.gen_uniform_u8x32().to_vec(),
            private_key,
            rp_id: String::from("example.com"),
            user_handle: vec![0x00],
            user_display_name: None,
            cred_protect_policy: Some(
                CredentialProtectionPolicy::UserVerificationOptionalWithCredentialIdList,
            ),
            creation_order: 0,
            user_name: None,
            user_icon: None,
        };
        assert!(persistent_store.store_credential(credential).is_ok());

        let no_credential = persistent_store
            .filter_credential("example.com", true)
            .unwrap();
        assert_eq!(no_credential, vec![]);
    }

    #[test]
    fn test_find() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        let credential_source0 = create_credential_source(&mut rng, "example.com", vec![0x00]);
        let credential_source1 = create_credential_source(&mut rng, "example.com", vec![0x01]);
        let id0 = credential_source0.credential_id.clone();
        let key0 = credential_source0.private_key.clone();
        assert!(persistent_store
            .store_credential(credential_source0)
            .is_ok());
        assert!(persistent_store
            .store_credential(credential_source1)
            .is_ok());

        let no_credential = persistent_store
            .find_credential("another.example.com", &id0, false)
            .unwrap();
        assert_eq!(no_credential, None);
        let found_credential = persistent_store
            .find_credential("example.com", &id0, false)
            .unwrap();
        let expected_credential = PublicKeyCredentialSource {
            key_type: PublicKeyCredentialType::PublicKey,
            credential_id: id0,
            private_key: key0,
            rp_id: String::from("example.com"),
            user_handle: vec![0x00],
            user_display_name: None,
            cred_protect_policy: None,
            creation_order: 0,
            user_name: None,
            user_icon: None,
        };
        assert_eq!(found_credential, Some(expected_credential));
    }

    #[test]
    fn test_find_with_cred_protect() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);
        assert_eq!(persistent_store.count_credentials().unwrap(), 0);
        let private_key = ctap_crypto::ecdsa::SecKey::gensk(&mut rng);
        let credential = PublicKeyCredentialSource {
            key_type: PublicKeyCredentialType::PublicKey,
            credential_id: rng.gen_uniform_u8x32().to_vec(),
            private_key,
            rp_id: String::from("example.com"),
            user_handle: vec![0x00],
            user_display_name: None,
            cred_protect_policy: Some(CredentialProtectionPolicy::UserVerificationRequired),
            creation_order: 0,
            user_name: None,
            user_icon: None,
        };
        assert!(persistent_store.store_credential(credential).is_ok());

        let no_credential = persistent_store
            .find_credential("example.com", &[0x00], true)
            .unwrap();
        assert_eq!(no_credential, None);
    }

    #[test]
    fn test_master_keys() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // Master keys stay the same within the same CTAP reset cycle.
        let master_keys_1 = persistent_store.master_keys().unwrap();
        let master_keys_2 = persistent_store.master_keys().unwrap();
        assert_eq!(master_keys_2.encryption, master_keys_1.encryption);
        assert_eq!(master_keys_2.hmac, master_keys_1.hmac);

        // Master keys change after reset. This test may fail if the random generator produces the
        // same keys.
        let master_encryption_key = master_keys_1.encryption.to_vec();
        let master_hmac_key = master_keys_1.hmac.to_vec();
        persistent_store.reset(&mut rng).unwrap();
        let master_keys_3 = persistent_store.master_keys().unwrap();
        assert!(master_keys_3.encryption != master_encryption_key.as_slice());
        assert!(master_keys_3.hmac != master_hmac_key.as_slice());
    }

    #[test]
    fn test_cred_random_secret() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // CredRandom secrets stay the same within the same CTAP reset cycle.
        let cred_random_with_uv_1 = persistent_store.cred_random_secret(true).unwrap();
        let cred_random_without_uv_1 = persistent_store.cred_random_secret(false).unwrap();
        let cred_random_with_uv_2 = persistent_store.cred_random_secret(true).unwrap();
        let cred_random_without_uv_2 = persistent_store.cred_random_secret(false).unwrap();
        assert_eq!(cred_random_with_uv_1, cred_random_with_uv_2);
        assert_eq!(cred_random_without_uv_1, cred_random_without_uv_2);

        // CredRandom secrets change after reset. This test may fail if the random generator produces the
        // same keys.
        persistent_store.reset(&mut rng).unwrap();
        let cred_random_with_uv_3 = persistent_store.cred_random_secret(true).unwrap();
        let cred_random_without_uv_3 = persistent_store.cred_random_secret(false).unwrap();
        assert!(cred_random_with_uv_1 != cred_random_with_uv_3);
        assert!(cred_random_without_uv_1 != cred_random_without_uv_3);
    }

    #[test]
    fn test_pin_hash() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // Pin hash is initially not set.
        assert!(persistent_store.pin_hash().unwrap().is_none());

        // Setting the pin hash sets the pin hash.
        let random_data = rng.gen_uniform_u8x32();
        assert_eq!(random_data.len(), 2 * PIN_AUTH_LENGTH);
        let pin_hash_1 = *array_ref!(random_data, 0, PIN_AUTH_LENGTH);
        let pin_hash_2 = *array_ref!(random_data, PIN_AUTH_LENGTH, PIN_AUTH_LENGTH);
        persistent_store.set_pin_hash(&pin_hash_1).unwrap();
        assert_eq!(persistent_store.pin_hash().unwrap(), Some(pin_hash_1));
        assert_eq!(persistent_store.pin_hash().unwrap(), Some(pin_hash_1));
        persistent_store.set_pin_hash(&pin_hash_2).unwrap();
        assert_eq!(persistent_store.pin_hash().unwrap(), Some(pin_hash_2));
        assert_eq!(persistent_store.pin_hash().unwrap(), Some(pin_hash_2));

        // Resetting the storage resets the pin hash.
        persistent_store.reset(&mut rng).unwrap();
        assert!(persistent_store.pin_hash().unwrap().is_none());
    }

    #[test]
    fn test_pin_retries() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // The pin retries is initially at the maximum.
        assert_eq!(persistent_store.pin_retries(), Ok(MAX_PIN_RETRIES));

        // Decrementing the pin retries decrements the pin retries.
        for pin_retries in (0..MAX_PIN_RETRIES).rev() {
            persistent_store.decr_pin_retries().unwrap();
            assert_eq!(persistent_store.pin_retries(), Ok(pin_retries));
        }

        // Decrementing the pin retries after zero does not modify the pin retries.
        persistent_store.decr_pin_retries().unwrap();
        assert_eq!(persistent_store.pin_retries(), Ok(0));

        // Resetting the pin retries resets the pin retries.
        persistent_store.reset_pin_retries().unwrap();
        assert_eq!(persistent_store.pin_retries(), Ok(MAX_PIN_RETRIES));
    }

    #[test]
    fn test_persistent_keys() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // Make sure the attestation are absent. There is no batch attestation in tests.
        assert!(persistent_store
            .attestation_private_key()
            .unwrap()
            .is_none());
        assert!(persistent_store
            .attestation_certificate()
            .unwrap()
            .is_none());

        // Make sure the persistent keys are initialized to dummy values.
        let dummy_key = [0x41u8; key_material::ATTESTATION_PRIVATE_KEY_LENGTH];
        let dummy_cert = [0xddu8; 20];
        persistent_store
            .set_attestation_private_key(&dummy_key)
            .unwrap();
        persistent_store
            .set_attestation_certificate(&dummy_cert)
            .unwrap();
        assert_eq!(&persistent_store.aaguid().unwrap(), key_material::AAGUID);

        // The persistent keys stay initialized and preserve their value after a reset.
        persistent_store.reset(&mut rng).unwrap();
        assert_eq!(
            &persistent_store.attestation_private_key().unwrap().unwrap(),
            &dummy_key
        );
        assert_eq!(
            persistent_store.attestation_certificate().unwrap().unwrap(),
            &dummy_cert
        );
        assert_eq!(&persistent_store.aaguid().unwrap(), key_material::AAGUID);
    }

    #[cfg(feature = "with_ctap2_1")]
    #[test]
    fn test_min_pin_length() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // The minimum PIN length is initially at the default.
        assert_eq!(
            persistent_store.min_pin_length().unwrap(),
            DEFAULT_MIN_PIN_LENGTH
        );

        // Changes by the setter are reflected by the getter..
        let new_min_pin_length = 8;
        persistent_store
            .set_min_pin_length(new_min_pin_length)
            .unwrap();
        assert_eq!(
            persistent_store.min_pin_length().unwrap(),
            new_min_pin_length
        );
    }

    #[cfg(feature = "with_ctap2_1")]
    #[test]
    fn test_min_pin_length_rp_ids() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        // The minimum PIN length RP IDs are initially at the default.
        assert_eq!(
            persistent_store._min_pin_length_rp_ids().unwrap(),
            _DEFAULT_MIN_PIN_LENGTH_RP_IDS
        );

        // Changes by the setter are reflected by the getter.
        let mut rp_ids = vec![String::from("example.com")];
        assert_eq!(
            persistent_store._set_min_pin_length_rp_ids(rp_ids.clone()),
            Ok(())
        );
        for rp_id in _DEFAULT_MIN_PIN_LENGTH_RP_IDS {
            if !rp_ids.contains(&rp_id) {
                rp_ids.push(rp_id);
            }
        }
        assert_eq!(persistent_store._min_pin_length_rp_ids().unwrap(), rp_ids);
    }

    #[test]
    fn test_global_signature_counter() {
        let mut rng = ThreadRng256 {};
        let mut persistent_store = PersistentStore::new(&mut rng);

        let mut counter_value = 1;
        assert_eq!(
            persistent_store.global_signature_counter().unwrap(),
            counter_value
        );
        for increment in 1..10 {
            assert!(persistent_store
                .incr_global_signature_counter(increment)
                .is_ok());
            counter_value += increment;
            assert_eq!(
                persistent_store.global_signature_counter().unwrap(),
                counter_value
            );
        }
    }

    #[test]
    fn test_serialize_deserialize_credential() {
        let mut rng = ThreadRng256 {};
        let private_key = ctap_crypto::ecdsa::SecKey::gensk(&mut rng);
        let credential = PublicKeyCredentialSource {
            key_type: PublicKeyCredentialType::PublicKey,
            credential_id: rng.gen_uniform_u8x32().to_vec(),
            private_key,
            rp_id: String::from("example.com"),
            user_handle: vec![0x00],
            user_display_name: None,
            cred_protect_policy: None,
            creation_order: 0,
            user_name: None,
            user_icon: None,
        };
        let serialized = serialize_credential(credential.clone()).unwrap();
        let reconstructed = deserialize_credential(&serialized).unwrap();
        assert_eq!(credential, reconstructed);
    }

    #[cfg(feature = "with_ctap2_1")]
    #[test]
    fn test_serialize_deserialize_min_pin_length_rp_ids() {
        let rp_ids = vec![String::from("example.com")];
        let serialized = _serialize_min_pin_length_rp_ids(rp_ids.clone()).unwrap();
        let reconstructed = _deserialize_min_pin_length_rp_ids(&serialized).unwrap();
        assert_eq!(rp_ids, reconstructed);
    }
}
