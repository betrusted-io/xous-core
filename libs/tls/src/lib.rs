pub mod cmd;
mod danger;
pub mod rota;
pub mod xtls;

use crate::rota::RustlsOwnedTrustAnchor;
use locales::t;
use modals::Modals;
use rkyv::{
    de::deserializers::AllocDeserializer,
    ser::{serializers::WriteSerializer, Serializer},
    Deserialize,
};
use rustls::{Certificate, ClientConfig, RootCertStore};
use sha2::Digest;
use std::convert::{Into, TryFrom, TryInto};
use std::io::{Error, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use x509_parser::prelude::{FromDer, X509Certificate};
use xous_names::XousNames;

/// PDDB Dict for tls trusted certificates keys
const TLS_TRUSTED_DICT: &str = "tls.trusted";

pub struct Tls {
    pddb: pddb::Pddb,
}

impl Tls {
    pub fn new() -> Tls {
        Tls {
            pddb: pddb::Pddb::new(),
        }
    }

    /// Presents a modal to the user to select trusted tls certificates
    /// and saves the selected certificates to the pddb
    ///
    /// # Arguments
    ///
    /// * `certificates` - the certificates to be presented
    ///
    ///  # Returns
    ///
    /// a count of trusted certificates
    ///
    pub fn check_trust(&self, certificates: &[Certificate]) -> usize {
        let xns = XousNames::new().unwrap();
        let modals = Modals::new(&xns).unwrap();

        let certificates: Vec<(String, X509Certificate)> = certificates
            .iter()
            .map(|cert| {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&cert);
                (
                    format!("{:X}", hasher.finalize()),
                    X509Certificate::from_der(cert.as_ref()),
                )
            })
            .filter(|(_fingerprint, result)| result.is_ok())
            .map(|(fingerprint, result)| (fingerprint, result.unwrap().1))
            .filter(|(_fingerprint, x509)| x509.is_ca())
            .collect();

        let chain: Vec<String> = certificates
            .iter()
            .map(|(fingerprint, x509)| format!("🏛 {}\n{}", &x509.subject(), open_hex(fingerprint),))
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
                    .map(|i| &certificates[*i].1)
                    .map(|x509| RustlsOwnedTrustAnchor::from(x509))
                    .for_each(|rota| {
                        self.save_cert(&rota).unwrap_or_else(|e| {
                            log::warn!("failed to save cert: {e}");
                            modals
                                .show_notification(
                                    format!("failed to save:\n{}\n{e}", &rota.subject()).as_str(),
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

    /// Deletes ALL tls trust-anchors from the pddb
    ///
    /// # Returns
    ///
    /// the number of trust-anchors deleted
    ///
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

    /// Deletes a tls trust-anchor from the pddb
    ///
    /// # Arguments
    ///
    /// * `key` - the pddb-key containing the unwanted trust-anchor
    ///
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

    /// Saves a tls trust-anchor to the pddb
    ///
    /// # Arguments
    ///
    /// * `ta` - a trusted trust-anchor
    ///
    pub fn save_cert(&self, ta: &RustlsOwnedTrustAnchor) -> Result<(), Error> {
        let key = ta.pddb_key();
        match self.pddb.get(
            TLS_TRUSTED_DICT,
            &key,
            None,
            true,
            true,
            Some(rota::MAX_ROTA_BYTES),
            None::<fn()>,
        ) {
            Ok(mut pddb_key) => {
                let mut buf = Vec::<u8>::new();
                // reserve 2 bytes to hold a u16 (see below)
                let reserved = 2;
                buf.push(0u8);
                buf.push(0u8);

                // serialize the trust-anchor
                let mut serializer = WriteSerializer::with_pos(buf, reserved);
                let pos = serializer.serialize_value(ta).unwrap();
                let mut bytes = serializer.into_inner();

                // copy pop u16 into the first 2 bytes to enable the rkyv archive to be deserialised
                let pos: u16 = u16::try_from(pos).expect("data > u16");
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
            Err(e) => log::warn!("failed to create {}:{}\n{}", TLS_TRUSTED_DICT, key, e),
        }
        Ok(())
    }

    /// Returns a tls trust-anchor from the pddb
    ///
    /// # Arguments
    ///
    /// * `key` - pddb key holding the trust-anchor
    ///
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
                        let archive =
                            unsafe { rkyv::archived_value::<RustlsOwnedTrustAnchor>(&bytes, pos) };
                        let ta = archive.deserialize(&mut AllocDeserializer {}).ok();
                        log::info!("get {}", key);
                        log::trace!("get '{}' = '{:?}'", key, &ta);
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

    /// Returns a Vec of all trusted trust-anchors
    ///
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

    /// Returns a RootCertStore containing all trusted trust-anchors
    ///
    pub fn root_store(&self) -> RootCertStore {
        let mut root_store = RootCertStore::empty();
        match self.pddb.list_keys(TLS_TRUSTED_DICT, None) {
            Ok(list) => {
                let rota = list
                    .iter()
                    .map(|key| self.get_cert(&key))
                    .filter_map(|rota| rota)
                    .map(|t| Into::<rustls::OwnedTrustAnchor>::into(t));
                root_store.add_trust_anchors(rota);
            }
            Err(e) => log::warn!("failed to get iter over trusted: {e}"),
        }
        root_store
    }

    /// Probes the target and returns a count of newly trusted 1 certificates
    ///
    /// Establishes a tls connection to the target host, extracts the
    /// certificates offered and immediately closes the connection.
    /// The certificates are presented by modal to the user, and saved to the
    /// pddb if trusted.
    ///
    /// By default, rustls only provides access to a trusted certificate chain.
    /// Probe briefly stifles the certificate validation (ie trusts everything)
    /// in order to grab the untrusted cetificate chain and present it to the
    /// user for examination.
    ///
    /// # Arguments
    ///
    /// * `target` - the target tls site (i.e. betrusted.io)
    ///
    pub fn probe(&self, target: &str) -> Result<usize, Error> {
        log::info!("starting TLS probe");
        // Attempt to open the tls connection with an empty root_store
        let root_store = rustls::RootCertStore::empty();
        // Stifle the default rustls certificate verification's complaint about an
        // unknown/untrusted CA root certificate so that we get to see the certificate chain
        let stifled_verifier =
            Arc::new(danger::StifledCertificateVerification { roots: root_store });
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(stifled_verifier)
            .with_no_client_auth();
        match target.try_into() {
            Ok(server_name) => {
                let mut conn =
                    rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();
                log::info!("connect TCPstream to {}", target);
                let url = format!("{}:443", target);
                match TcpStream::connect(url) {
                    Ok(mut sock) => {
                        match conn.complete_io(&mut sock) {
                            Ok(_) => log::info!("handshake complete"),
                            Err(e) => log::warn!("{e}"),
                        }
                        conn.send_close_notify();
                        match conn.peer_certificates() {
                            Some(certificates) => Ok(self.check_trust(certificates)),
                            None => Ok(0),
                        }
                    }
                    Err(e) => {
                        log::warn!("{e}");
                        Err(e)
                    }
                }
            }
            Err(e) => {
                log::warn!("failed to create sever_name from {target}: {e}");
                Err(Error::from(ErrorKind::InvalidInput))
            }
        }
    }

    pub fn client_config(&self) -> ClientConfig {
        rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(self.root_store())
            .with_no_client_auth()
    }
}

// https://stackoverflow.com/questions/57029974/how-to-split-string-into-chunks-in-rust-to-insert-spaces
// insert a space between each hex value
fn open_hex(text: &str) -> String {
    text.chars()
        .enumerate()
        .flat_map(|(i, c)| {
            if i != 0 && i % 2 == 0 {
                Some(' ')
            } else {
                None
            }
            .into_iter()
            .chain(std::iter::once(c))
        })
        .collect::<String>()
}
