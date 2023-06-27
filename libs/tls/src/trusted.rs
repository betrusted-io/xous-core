use crate::get_cert;
use crate::{RustlsOwnedTrustAnchor, TLS_CERT_DICT};
use rustls::RootCertStore;
use std::io::{Error, ErrorKind, Read, Write};
use std::path::PathBuf;

pub struct Trusted {
    trusted: std::fs::ReadDir,
}

impl Trusted {
    pub fn new() -> Result<Trusted, Error> {
        let mut keypath = PathBuf::new();
        keypath.push(TLS_CERT_DICT);
        if !std::fs::metadata(&keypath).is_ok() {
            log::info!("dict '{}' does NOT exist.. creating it", TLS_CERT_DICT);
            std::fs::create_dir_all(&keypath)?;
        }
        Ok(Self {
            trusted: std::fs::read_dir(keypath)?,
        })
    }
}

impl Iterator for Trusted {
    type Item = RustlsOwnedTrustAnchor;

    fn next(&mut self) -> Option<Self::Item> {
        match self.trusted.next() {
            Some(Ok(dir_entry)) => match dir_entry.file_name().into_string() {
                Ok(key) => {
                    log::info!("path: {}", key);
                    get_cert(&key).unwrap_or(None)
                }
                Err(e) => {
                    log::warn!("failed to read cert: {:?}", e);
                    None
                }
            },
            Some(Err(e)) => {
                log::warn!("failed to list trusted certs: {}", e);
                None
            }
            None => None,
        }
    }
}

impl Into<RootCertStore> for Trusted {
    fn into(self) -> RootCertStore {
        let mut root_store = rustls::RootCertStore::empty();
        let trusted = Trusted::new().unwrap();
        root_store
            .add_server_trust_anchors(trusted.map(|t| Into::<rustls::OwnedTrustAnchor>::into(t)));
        root_store
    }
}
