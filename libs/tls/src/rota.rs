// A mirror of rustls::OwnedTrustAnchor

use der::{Encode, Reader};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::min;
use std::convert::TryInto;
use std::fmt;
use std::io::{Error, ErrorKind};
use x509_parser::der_parser::der::Tag;
use x509_parser::prelude::{FromDer, X509Certificate};

/// A close mirror of rustls::OwnedTrustAnchor - but with extras
/// Note that the subject, spki & name_constraints fields are
/// DER encoded but WITHOUT the DER header.
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
            name_constraints: name_constraints.map(|x| x.into()),
        }
    }

    pub fn subject(&self) -> String {
        let der = self.subject.clone();
        let len = min(der.len(), 127);
        let mut der = der[..len].to_owned();
        // hack back in the DER header to enable decode :-/
        der.insert(0, len as u8);
        der.insert(0, Tag::Sequence.0 as u8);
        match x509_parser::x509::X509Name::from_der(&der) {
            Ok((_, decoded)) => decoded.to_string(),
            Err(e) => format!("der parse failed: {e}").to_string(),
        }
    }
}

impl<'a> From<&X509Certificate<'a>> for RustlsOwnedTrustAnchor {
    fn from(x509: &X509Certificate) -> Self {
        // Remove the DER headers in keeping with rustls::OwnersTrustAnchor
        RustlsOwnedTrustAnchor {
            subject: match rm_der_header(x509.subject().as_raw()) {
                Ok(naked_der) => naked_der,
                Err(e) => {
                    log::warn!("{e}");
                    b"der decode failed".to_vec()
                }
            },
            spki: match rm_der_header(x509.public_key().raw) {
                Ok(naked_der) => naked_der,
                Err(e) => {
                    log::warn!("{e}");
                    b"der decode failed".to_vec()
                }
            },
            name_constraints: None,
            // name_constraints: x509.name_constraints().unwrap_or(None).map(|c| c.value.into()),
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

/// Remove the DER header from the DER encoded [u8]
/// Remove a DER header from a DER encoded [u8]
fn rm_der_header(der: &[u8]) -> Result<Vec<u8>, Error> {
    match der::SliceReader::new(der) {
        Ok(reader) => match reader.peek_header() {
            Ok(header) => match header.encoded_len() {
                Ok(len) => match TryInto::<usize>::try_into(len) {
                    Ok(len) => Ok(der[len..].to_vec()),
                    Err(_) => Err(Error::new(
                        ErrorKind::InvalidData,
                        "der decode failed: into",
                    )),
                },
                Err(_) => Err(Error::new(
                    ErrorKind::InvalidData,
                    "der decode failed: length",
                )),
            },
            Err(_) => Err(Error::new(
                ErrorKind::InvalidData,
                "der decode failed: header",
            )),
        },
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "der decode failed: reader",
        )),
    }
}

impl<'a> From<&webpki::TrustAnchor<'a>> for RustlsOwnedTrustAnchor {
    fn from(ta: &webpki::TrustAnchor) -> Self {
        Self::from_subject_spki_name_constraints(ta.subject, ta.spki, ta.name_constraints)
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
