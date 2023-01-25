use std::io::{Read, Write};
use vault::ctap::storage::key;
use std::num::ParseIntError;
use pddb::Pddb;

const FIDO_DICT: &'static str = "fido.cfg";
const FIDO_CRED_DICT: &'static str = "fido.cred";
const FIDO_PERSISTENT_DICT: &'static str = "fido.persistent";

fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}
/// The point of the attestation key is to delegate the question of the authenticity of your U2F
/// key effectively to a transaction that happens between the factory and the server. If the
/// attestation private key is an impenetrable secret and you can trust the factory, then the
/// attestation key effectively proves the U2F token was made by the factory to its specifications.
///
/// In the case of Precursor's threat model, you don't trust the factory, and you assume there is
/// basically no such thing as an impenetrable secret. So an attestation key is equivalent to any
/// private key created entirely within your own device; its use proves that you're you, or at
/// least, someone has possession of your device and your unlock password.
///
/// Thus, for Precursor, there is no delegation of the question of authenticity -- you
/// choose to verify the device to the level you're satisfied with. If you're happy to
/// just take it out of the box and use it without checking anything, that's your choice,
/// but you can also check everything as much as you like.
///
/// As a result, the "attestation key" serves no role for Precursor users, other than to
/// identify them as Precursor users. A unique per-device key would furthermore allow
/// a server to uniquely identify each Precursor user. Therefore, we commit the private key
/// into this public repository so any and all Precursor users may share it. The more
/// widely it is used, the harder it becomes to de-anonymize a Precursor users.
///
/// This key was generated using the script in `tools/gen_key_materials.sh`, and then printed
/// out by calling
/// `./configure.py --private-key=crypto_data/opensk.key --certificate=crypto_data/opensk_cert.pem`
pub fn migrate(pddb: &Pddb) -> Result<(), xous::Error> {
    if !pddb.list_dict(None).unwrap().contains(&persistent_store::store::OPENSK2_DICT.to_string()) {
        // build the attestation keys
        let pk = decode_hex("b8c3abd05cbe17b2faf87659c6f73e8467832112a0e609807cb68996c9a0c6a8").unwrap();
        pddb.get(
            persistent_store::store::OPENSK2_DICT,
            &vault::api::attestation_store::STORAGE_KEYS[0].to_string(),
            None,
            true, true, Some(32), None::<fn()>
        ).unwrap().write_all(&pk).map_err(|_| xous::Error::AccessDenied)?;
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
        pddb.get(
            persistent_store::store::OPENSK2_DICT,
            &vault::api::attestation_store::STORAGE_KEYS[1].to_string(),
            None,
            true, true, None, None::<fn()>
        ).unwrap().write_all(&der).map_err(|_| xous::Error::AccessDenied)?;

        // port the master secret
        migrate_one(pddb, FIDO_DICT, "CRED_RANDOM_SECRET", key::CRED_RANDOM_SECRET)?;
        // port AAGUID
        migrate_one(pddb, FIDO_PERSISTENT_DICT, "AAGUID", key::AAGUID)?;
        // port master keys
        migrate_one(pddb, FIDO_DICT, "MASTER_KEYS", key::_RESERVED_KEY_STORE)?;

        // port other settings
        // Turns out that porting over the PIN is a mistake. Just clear it.

        // migrate_one(pddb, FIDO_DICT, "MIN_PIN_LENGTH", key::MIN_PIN_LENGTH)?;
        // migrate_one(pddb, FIDO_DICT, "PIN_RETRIES", key::PIN_RETRIES)?;
        migrate_one(pddb, FIDO_DICT, "GLOBAL_SIGNATURE_COUNTER", key::GLOBAL_SIGNATURE_COUNTER)?;
        // PIN_HASH requires appending a length to the end. This is an `advisory` field, so we...just make up a value of 8.
        // This may not actually be the length used, but it's likely to be long enough to avoid forcing a re-PIN event because
        // the PIN hash is too short.
        // migrate_pin(pddb)?;

        // migrate credentials
        let creds = match pddb.list_keys(FIDO_CRED_DICT, None) {
            Ok(list) => list,
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Vec::new(),
                _ => return Err(xous::Error::InternalError)
            }
        };
        let expected_creds = creds.len();
        let mut successful_migrations = 0;
        let cred_start = key::CREDENTIALS.start;
        for cred in creds {
            let mut key = pddb.get(
                FIDO_CRED_DICT,
                &cred,
                None, false, false, None, None::<fn()>
            ).map_err(|_| xous::Error::AccessDenied)?;
            let mut data = Vec::<u8>::new();
            key.read_to_end(&mut data).unwrap();
            match vault::ctap::storage::deserialize_credential(&data) {
                Some(pk) => {
                    log::debug!("Valid pk: {:?}", pk);
                    let migrated_data = vault::ctap::storage::serialize_credential(pk).map_err(|_| xous::Error::InternalError)?;
                    let mut new_key = pddb.get(
                        persistent_store::store::OPENSK2_DICT,
                        &(cred_start + successful_migrations).to_string(),
                        None, true, true, Some(migrated_data.len()), None::<fn()>
                    ).map_err(|_| xous::Error::AccessDenied)?;
                    new_key.write_all(&migrated_data).map_err(|_| xous::Error::OutOfMemory)?;
                    successful_migrations += 1;
                    log::info!("Migrated credential {}:{}", FIDO_CRED_DICT, cred);
                }
                None => {
                    log::warn!("Credential {}:{} did not deserialize, cannot migrate!", FIDO_CRED_DICT, cred);
                }
            }
        }
        pddb.sync().ok();
        if successful_migrations == expected_creds {
            Ok(())
        } else {
            Err(xous::Error::InvalidString)
        }
    } else {
        // OpenSK dict already exists; don't migrate anything.
        Ok(())
    }
}

/// Only migrates the data if it exists. Does not return an error if the record does not exist
fn migrate_one (
    pddb: &Pddb,
    legacy_dict: &str,
    legacy_key: &str,
    new_key: usize,
) -> Result<(), xous::Error> {
    match pddb.get(
        legacy_dict,
        legacy_key,
        None, false, false, None, None::<fn()>
    ) {
        Ok(mut key) => {
            let mut data = Vec::<u8>::new();
            let len = key.read_to_end(&mut data).map_err(|_| xous::Error::AccessDenied)?;
            let mut new_key = pddb.get(
                persistent_store::store::OPENSK2_DICT,
                &new_key.to_string(),
                None, true, true, Some(len), None::<fn()>
            ).map_err(|_| xous::Error::AccessDenied)?;
            new_key.write_all(&data).map_err(|_| xous::Error::OutOfMemory)?;
            log::info!("successfully migrated {}:{}", legacy_dict, legacy_key);
            Ok(())
        },
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    log::info!("migration skipping {}:{}, as it does not exist", legacy_dict, legacy_key);
                    Ok(())
                },
                _ => Err(xous::Error::InternalError)
            }
        }
    }
}

/// Handle the special case of the PIN_HASH
fn migrate_pin (
    pddb: &Pddb,
) -> Result<(), xous::Error> {
    let legacy_dict = FIDO_DICT;
    let legacy_key = "PIN_HASH";
    let new_key = key::PIN_PROPERTIES;
    match pddb.get(
        legacy_dict,
        legacy_key,
        None, false, false, None, None::<fn()>
    ) {
        Ok(mut key) => {
            let mut data = Vec::<u8>::new();
            let len = key.read_to_end(&mut data).map_err(|_| xous::Error::AccessDenied)? + 1;
            data.push(5); // add a bogus "length" advisory field of 8
            let mut new_key = pddb.get(
                persistent_store::store::OPENSK2_DICT,
                &new_key.to_string(),
                None, true, true, Some(len), None::<fn()>
            ).map_err(|_| xous::Error::AccessDenied)?;
            new_key.write_all(&data).map_err(|_| xous::Error::OutOfMemory)?;
            log::info!("successfully migrated {}:{}", legacy_dict, legacy_key);
            Ok(())
        },
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    log::info!("migration skipping {}:{}, as it does not exist", legacy_dict, legacy_key);
                    Ok(())
                },
                _ => Err(xous::Error::InternalError)
            }
        }
    }
}