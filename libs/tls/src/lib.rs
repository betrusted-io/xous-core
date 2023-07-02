pub mod cmd;
pub mod danger;
pub mod rota;

use crate::rota::RustlsOwnedTrustAnchor;
use locales::t;
use modals::Modals;
use rkyv::{
    de::deserializers::AllocDeserializer,
    ser::{serializers::WriteSerializer, Serializer},
    Deserialize,
};
use rustls::{Certificate, RootCertStore};
use std::convert::{Into, TryFrom};
use std::io::{Error, ErrorKind, Read, Write};
use x509_parser::prelude::{oid_registry, FromDer, X509Certificate};
use xous_names::XousNames;

/// PDDB Dict for tls trusted certificates keys
const TLS_TRUSTED_DICT: &str = "tls.trusted";
const CURRENT_VERSION_KEY: &str = "__version";

pub struct Tls {
    pddb: pddb::Pddb,
}

impl Tls {
    pub fn new() -> Tls {
        Tls {
            pddb: pddb::Pddb::new(),
        }
    }

    // presents a modal to the user to select trusted tls certificates
    // and saves the selected certificates to the pddb
    // returns a count of trusted certificates
    pub fn check_trust(&self, certificates: &[Certificate]) -> usize {
        let xns = XousNames::new().unwrap();
        let modals = Modals::new(&xns).unwrap();

        let certificates: Vec<X509Certificate> = certificates
            .iter()
            .map(|cert| X509Certificate::from_der(cert.as_ref()))
            .filter(|result| result.is_ok())
            .map(|result| result.unwrap().1)
            .filter(|x509| x509.is_ca())
            .collect();

        let chain: Vec<String> = certificates
            .iter()
            .map(|x509| {
                let subject = x509.subject();
                format!(
                    "{}{}\n{}",
                    if x509.is_ca() { "🏛 " } else { "" },
                    &subject,
                    "sha256 fingerprint here",
                )
            })
            .collect();
        let chain: Vec<&str> = chain.iter().map(AsRef::as_ref).collect();
        modals
            .add_list(chain)
            .expect("couldn't build checkbox list");
        match modals.get_checkbox(t!("tls.check_trust_prompt", locales::LANG)) {
            Ok(trusted) => {
                trusted
                    .iter()
                    .for_each(|cert| log::info!("trusts {}", cert));
                modals
                    .get_check_index()
                    .unwrap()
                    .iter()
                    .map(|i| &certificates[*i])
                    .map(|x509| {
                        let subject = &x509
                            .subject()
                            .to_string_with_registry(oid_registry())
                            .unwrap();
                        (subject.to_owned(), RustlsOwnedTrustAnchor::from(x509))
                    })
                    .for_each(|(key, val)| {
                        self.save_cert(&key, &val).unwrap_or_else(|e| {
                            log::warn!("failed to save cert: {e}");
                            modals
                                .show_notification(
                                    format!("failed to save:\n{}\n{e}", &val.subject()).as_str(),
                                    None,
                                )
                                .expect("modal failed");
                        });
                    });
                trusted.len()
            }
            _ => {
                log::error!("get_checkbox failed");
                0
            }
        }
    }

    // deletes ALL tls trust-anchors from the pddb
    // returns the number of certs deleted
    pub fn del_all_cert(&self) -> Result<usize, Error> {
        let count = match self.pddb.list_keys(TLS_TRUSTED_DICT, None) {
            Ok(list) => list.len(),
            Err(_) => 0,
        };
        match self.pddb.delete_dict(TLS_TRUSTED_DICT, None) {
            Ok(_) => {
                log::info!("Deleted {}\n", TLS_TRUSTED_DICT);
                self.pddb
                    .sync()
                    .or_else(|e| Ok::<(), Error>(log::warn!("{e}")))
                    .ok();
                // match self.pddb.sync() {
                //     Err(e) => log::warn!("{e}"),
                //     _ => (),
                // }
            }
            Err(e) => log::warn!("failed to delete {}: {:?}", TLS_TRUSTED_DICT, e),
        }
        Ok(count)
    }

    // deletes a tls trust-anchor from the pddb
    pub fn del_cert(&self, key: &str) -> Result<(), Error> {
        match self.pddb.delete_key(TLS_TRUSTED_DICT, key, None) {
            Ok(_) => {
                log::info!("Deleted {}:{}\n", TLS_TRUSTED_DICT, key);
                self.pddb
                    .sync()
                    .or_else(|e| Ok::<(), Error>(log::warn!("{e}")))
                    .ok();
            }
            Err(e) => log::warn!("failed to delete {}:{}: {:?}", TLS_TRUSTED_DICT, key, e),
        }
        return Ok(());
    }

    // saves a tls trust-anchor to the pddb
    pub fn save_cert(&self, key: &str, ta: &RustlsOwnedTrustAnchor) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "may not set a variable beginning with __ ",
            ))
        } else {
            match self.pddb.get(
                TLS_TRUSTED_DICT,
                key,
                None,
                true,
                true,
                Some(rota::MAX_ROTA_BYTES),
                None::<fn()>,
            ) {
                Ok(mut pddb_key) => {
                    let mut buf = Vec::<u8>::new();
                    // reserve 2 bytes to hold a u16 (see below)
                    buf.push(0u8);
                    buf.push(0u8);

                    // serialize the trust-anchor
                    let mut serializer = WriteSerializer::new(buf);
                    let pos = serializer.serialize_value(ta).unwrap();
                    let mut bytes = serializer.into_inner();

                    // copy pop u16 into the first 2 bytes to enable the rkyv archive to be deserialised
                    let pos:u16 = u16::try_from(pos).expect("data > u16");
                    let pos_bytes = pos.to_be_bytes();
                    bytes[0] = pos_bytes[0];
                    bytes[1] = pos_bytes[1];

                    match pddb_key.write(&bytes) {
                        Ok(len) => {
                            self.pddb.sync().ok();
                            log::info!("Wrote {} bytes to {}:{}", len, TLS_TRUSTED_DICT, key);
                        }
                        Err(e) => {
                            log::warn!("Error writing {}:{}: {:?}", TLS_TRUSTED_DICT, key, e);
                        }
                    }
                }
                _ => log::warn!("failed to create {}:{}", TLS_TRUSTED_DICT, key),
            }
            Ok(())
        }
    }


    // retrieves a tls trust-anchor from the pddb
    pub fn get_cert(&self, key: &str) -> Option<RustlsOwnedTrustAnchor> {
        match self.pddb.get(
            TLS_TRUSTED_DICT,
            key,
            None,
            false,
            false,
            None,
            None::<fn()>,
        ) {
            Ok(mut pddb_key) => {
                let mut bytes = [0u8; rota::MAX_ROTA_BYTES];
                match pddb_key.read(&mut bytes) {
                    Ok(_) => {
                        // extract pos u16 from the first 2 bytes
                        let pos: u16 = u16::from_be_bytes([bytes[0], bytes[1]]);
                        let pos: usize = pos.into();
                        // deserialize the trust-anchor
                        let archive = unsafe {
                            rkyv::archived_value::<RustlsOwnedTrustAnchor>(&bytes[2..2+pos], pos)
                        };
                        let ta = archive.deserialize(&mut AllocDeserializer {}).ok();
                        log::info!("get '{}' = '{:?}'", key, &ta);
                        ta
                    }
                    Err(e) => {
                        log::warn!("failed to read {}: {e}", key);
                        return None;
                    }
                }
            }
            Err(e) => {
                log::warn!("failed to get {}: {e}", key);
                return None;
            }
        }
    }

    pub fn trusted(&self) -> Vec<RustlsOwnedTrustAnchor> {
        match self.pddb.list_keys(TLS_TRUSTED_DICT, None) {
            Ok(list) => list
                .iter()
                .map(|key| self.get_cert(&key))
                .filter_map(|rota| rota)
                .collect::<Vec<RustlsOwnedTrustAnchor>>(),
            Err(e) => {
                log::warn!("failed to get iter over trusted: {e}");
                Vec::<RustlsOwnedTrustAnchor>::new()
            }
        }
    }

    pub fn root_store(&self) -> RootCertStore {
        let mut root_store = RootCertStore::empty();
        match self.pddb.list_keys(TLS_TRUSTED_DICT, None) {
            Ok(list) => {
                let rota = list
                    .iter()
                    .map(|key| self.get_cert(&key))
                    .filter_map(|rota| rota)
                    .map(|t| Into::<rustls::OwnedTrustAnchor>::into(t));
                root_store.add_server_trust_anchors(rota);
            }
            Err(e) => log::warn!("failed to get iter over trusted: {e}"),
        }
        root_store
    }
}












