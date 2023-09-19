// A mirror of rustls::OwnedTrustAnchor

use der::{Encode, Header, Reader, Tag};
use rkyv::{Archive, Deserialize, Serialize};
use std::cmp::min;
use std::convert::TryInto;
use std::io::{Error, ErrorKind};
use x509_parser::prelude::{FromDer, X509Certificate};

pub const MAX_ROTA_BYTES: usize = 1028;

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

    // decoded subject
    pub fn subject(&self) -> String {
        match add_der_header(Tag::Sequence, &self.subject) {
            Ok(der) => match x509_parser::x509::X509Name::from_der(&der) {
                Ok((_, decoded)) => decoded.to_string(),
                Err(e) => {
                    log::warn!("{:?}", e);
                    "der decode failed".to_string()
                }
            },
            Err(e) => {
                log::warn!("{:?}", e.into_inner().unwrap());
                "der decode failed".to_string()
            }
        }
    }

    // decoded public key
    // see https://docs.rs/x509-parser/0.15.0/src/x509_parser/x509.rs.html#260-274
    #[allow(unreachable_code)]
    pub fn public_key(&self) -> Result<Vec<u8>, Error> {
        match add_der_header(Tag::Sequence, &self.spki) {
            Ok(der) => match x509_parser::x509::SubjectPublicKeyInfo::from_der(&der) {
                Ok((_, spki)) => Ok(spki.subject_public_key.data.to_vec()),
                Err(e) => {
                    log::warn!("{:?}", e);
                    Err(Error::new(ErrorKind::InvalidData, "failed spki der decode"))
                }
            },
            Err(e) => {
                log::warn!("{:?}", e.into_inner().unwrap());
                Err(Error::new(ErrorKind::InvalidData, "failed spki der header"))
            }
        }
    }

    pub fn pddb_key(&self) -> String {
        let subject = &self.subject();
        let begin = match subject.find("CN=") {
            Some(begin) => Some(begin),
            None => subject.find("OU="),
        };

        let mut pddb_key = match begin {
            Some(mut begin) => {
                begin += 3;
                let end = match subject[begin..].find(",") {
                    Some(e) => begin + e,
                    None => subject.len(),
                };
                &subject[begin..end]
            }
            None => {
                log::warn!("Subject missing CN= & OU= :{}", &subject);
                &subject
            }
        }
        .to_string();

        // grab a few arbitrary bytes from spki so pddb_key is deterministic & unique
        let k = &self.spki;
        pddb_key.push_str(&format!(" {:X}{:X}{:X}{:X}", k[6], k[7], k[8], k[9]));

        // mirror of pddb::KEY_NAME_LEN
        // u64: vaddr/len/resvd, u32: flags, age = 95
        // would this be better as a pddb pub?
        const KEY_NAME_LEN: usize = 127 - 8 - 8 - 8 - 4 - 4;
        pddb_key[..min(pddb_key.len(), KEY_NAME_LEN - 1)].to_string()
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
            name_constraints: match x509.name_constraints() {
                Ok(Some(_)) => {
                    log::warn!("Name Constraints ignored");
                    None
                }
                Ok(None) => None,
                Err(e) => {
                    log::warn!("{e}");
                    None
                }
            },
        }
    }
}

/// Add a DER header to a DER encoded [u8]
fn add_der_header(tag: Tag, naked: &Vec<u8>) -> Result<Vec<u8>, Error> {
    match Header::new(tag, naked.len()) {
        Ok(header) => {
            let mut buff: [u8; 32] = [0u8; 32];
            match header.encode_to_slice(&mut buff) {
                Ok(der) => {
                    Ok([der, naked].concat())
                }
                Err(_) => Err(Error::new(
                    ErrorKind::InvalidData,
                    "der parse failed: encode",
                )),
            }
        }
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "der parse failed: header",
        )),
    }
}

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
