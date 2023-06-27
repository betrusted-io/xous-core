pub mod cmd;
pub mod danger;

use modals::Modals;
use rkyv::{
    de::deserializers::AllocDeserializer,
    ser::{serializers::WriteSerializer, Serializer},
    AlignedVec, Archive, Deserialize, Serialize,
};
use rustls::{Certificate, RootCertStore};
use std::cmp::min;
use std::convert::{Into, TryFrom};
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::path::PathBuf;
use x509_parser::prelude::{FromDer, X509Certificate};
use x509_parser::der_parser::der::Tag;
use xous_names::XousNames;

/// PDDB Dict for tls trusted certificates keys
const TLS_CERT_DICT: &str = "tls/cert";
const CURRENT_VERSION_KEY: &str = "__version";

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct RustlsOwnedTrustAnchor {
    pub subject: Vec<u8>,
    pub spki: Vec<u8>,
    pub name_constraints: Option<Vec<u8>>,
}

impl RustlsOwnedTrustAnchor {

pub fn from_subject_spki_name_constraints(
    subject: impl Into<Vec<u8>>,
    spki: impl Into<Vec<u8>>,
    name_constraints: Option<impl Into<Vec<u8>>>,
) -> Self {
    Self { 
        subject: subject.into(), 
        spki: spki.into(), 
        name_constraints: name_constraints.map(|x| x.into()) 
    }
}


    pub fn subject(&self) -> String {
        let der = self.subject.clone();
        let len = min(der.len(), 127);
        let mut der = der[..len].to_owned();
        der.insert(0, len as u8);
        der.insert(0, Tag::Sequence.0 as u8);
        match x509_parser::x509::X509Name::from_der(&der) {
            Ok((_, decoded)) => decoded.to_string(),
            Err(e) => format!("der parse failed: {e}").to_string(),
        }
    }



    }
}
impl<'a> From<&X509Certificate<'a>> for RustlsOwnedTrustAnchor {
    fn from(x509: &X509Certificate) -> Self {
        RustlsOwnedTrustAnchor {
            subject: x509.subject().as_raw().to_vec(),
            spki: x509.public_key().raw.to_vec(),
            name_constraints: None::<Vec<u8>>,
            // may have to pass value thru from certificates parameter
            // name_constraints: match x509.name_constraints() {
            //     Ok(Some(nc)) => Some(nc.value),
            //     Ok(None) => None,
            //     Err(e) => {
            //         log::warn!("failed to extract x509 name_constraints: {}", e);
            //         None
            //     }
            // },
        }
    }
}

impl Into<rustls::OwnedTrustAnchor> for RustlsOwnedTrustAnchor {
    fn into(self) -> rustls::OwnedTrustAnchor {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            self.subject,
            self.spki,
            self.name_constraints,
        )
    }
}

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

// presents a modal to the user to select trusted tls certificates
// and saves the selected certificates to the pddb
// returns false if no certificates are trusted - and true otherwise
pub fn check_trust(certificates: &[Certificate]) -> bool {
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
                "{}{}\n{}\n{}",
                if x509.is_ca() { "🏛 " } else { "" },
                &subject,
                &x509.raw_serial_as_string()[0..24],
                &x509.raw_serial_as_string()[24..],
            )
        })
        .collect();
    let chain: Vec<&str> = chain.iter().map(AsRef::as_ref).collect();
    modals
        .add_list(chain)
        .expect("couldn't build checkbox list");
    match modals.get_checkbox("Do you trust any of these certificate authorities?") {
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
                    (
                        x509.raw_serial_as_string(),
                        RustlsOwnedTrustAnchor::from(x509),
                    )
                })
                .for_each(|(key, val)| {
                    save_cert(&key, &val).unwrap_or_else(|e| {
                        log::warn!("failed to save cert: {e}");
                        modals
                            .show_notification(
                                format!("failed to save: {}", &val.subject()).as_str(),
                                None,
                            )
                            .expect("modal failed");
                    });
                });
            trusted.len() > 0
        }
        _ => {
            log::error!("get_checkbox failed");
            false
        }
    }
}

// saves a tls trust-anchor to the pddb
pub fn save_cert(key: &str, ta: &RustlsOwnedTrustAnchor) -> Result<(), Error> {
    if key.starts_with("__") {
        Err(Error::new(
            ErrorKind::PermissionDenied,
            "may not set a variable beginning with __ ",
        ))
    } else {
        log::trace!("set '{}' = '{:?}'", key, ta);
        let mut keypath = PathBuf::new();
        keypath.push(TLS_CERT_DICT);
        if !std::fs::metadata(&keypath).is_ok() {
            log::info!("dict '{}' does NOT exist.. creating it", TLS_CERT_DICT);
            std::fs::create_dir_all(&keypath)?;
        }
        keypath.push(key);

        // serialize the trust-anchor
        let mut serializer = WriteSerializer::new(AlignedVec::new());
        let pos: u16 = u16::try_from(serializer.serialize_value(ta).unwrap()).expect("data > u16");

        // this next bit of black-magic bolts pos onto buff to enable deserialization
        let mut buf = serializer.into_inner();
        let pos_bytes = pos.to_be_bytes();
        buf.push(pos_bytes[0]);
        buf.push(pos_bytes[1]);

        // save key & trust-anchor to pddb
        File::create(keypath)?.write_all(&buf[..])?;
        Ok(())
    }
}

// retrieves a tls trust-anchor from the pddb
pub fn get_cert(key: &str) -> Result<Option<RustlsOwnedTrustAnchor>, Error> {
    let mut keypath = PathBuf::new();
    keypath.push(TLS_CERT_DICT);
    if !std::fs::metadata(&keypath).is_ok() {
        log::info!("dict '{}' does NOT exist.. creating it", TLS_CERT_DICT);
        std::fs::create_dir_all(&keypath)?;
    }

    keypath.push(key);
    if let Ok(mut file) = File::open(keypath) {
        // read the key & serialized trust-anchor from the pddb
        let mut bytes = Vec::<u8>::new();
        let len = file.read_to_end(&mut bytes)?;

        // black unmagic to unbold pos from the buffer end
        let pos: u16 = u16::from_be_bytes([bytes[len - 2], bytes[len - 1]]);

        // deserialize the trust-anchor
        let archive = unsafe { rkyv::archived_value::<RustlsOwnedTrustAnchor>(&bytes, pos.into()) };
        let ta = archive.deserialize(&mut AllocDeserializer {}).ok();

        log::trace!("get '{}' = '{:?}'", key, ta);
        return Ok(ta);
    } else {
        return Ok(None);
    }
}
