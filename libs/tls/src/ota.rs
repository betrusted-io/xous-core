// A rkyv serialisable intermediatory for a TrustAnchor
use der::{Encode, Header, Reader, Tag};
use rkyv::{Archive, Deserialize, Serialize};
use rustls::pki_types::{Der, TrustAnchor};
use std::cmp::min;
use std::convert::TryInto;
use std::fmt;
use std::io::{Error, ErrorKind};
use x509_parser::prelude::{FromDer, X509Certificate};
use x509_parser::x509::X509Name;

pub const MAX_OTA_BYTES: usize = 1028;

/// Note that the subject, spki & name_constraints fields are all DER encoded,
/// but WITHOUT the DER header, in keeping with webpki-roots.
#[derive(Archive, Serialize, Deserialize)]
pub struct OwnedTrustAnchor {
    pub subject: Vec<u8>,
    pub spki: Vec<u8>,
    pub name_constraints: Option<Vec<u8>>,
}

impl OwnedTrustAnchor {
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

    /// Creates a OwnedTrustAnchor from an x509 Certificate
    ///
    /// The Der headers are removed from each of the fields during the conversion.
    ///
    /// # Arguments
    ///
    /// * `x509` - a X509Certificate to create the OwnedTrustAnchor
    ///
    /// # Returns
    ///
    /// An OwnedTrustAnchor based on the supplied X509Certificate
    ///
    pub fn from_x509(x509: &X509Certificate) -> Result<Self, Error> {
        match (
            rm_der_header(x509.subject().as_raw()),
            rm_der_header(x509.public_key().raw),
            x509.name_constraints(),
        ) {
            (Ok(subject), Ok(spki), Ok(name_constraints)) => match name_constraints {
                // Ignore name_constrains for now TODO
                // The problem is that it is hard to get access to the raw &[u8] form of name_constraints
                Some(_) => Ok(Self::from_subject_spki_name_constraints(subject, spki, None::<&[u8]>)),
                None => Ok(Self::from_subject_spki_name_constraints(subject, spki, None::<&[u8]>)),
            },
            (Err(e), _, _) => {
                log::warn!("failed to remove header from subject: {e}");
                Err(Error::from(ErrorKind::InvalidData))
            }
            (_, Err(e), _) => {
                log::warn!("failed to remove header from subject_public_key_info: {e}");
                Err(Error::from(ErrorKind::InvalidData))
            }
            (_, _, Err(e)) => {
                log::warn!("failed to extract name_constraints: {e}");
                Err(Error::from(ErrorKind::InvalidData))
            }
        }
    }

    pub fn pddb_key(&self) -> Result<String, Error> {
        match self.subject() {
            Ok(subject) => {
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
                Ok(pddb_key[..min(pddb_key.len(), KEY_NAME_LEN - 1)].to_string())
            }
            Err(e) => {
                log::warn!("failed to construct pddb_key: {e}");
                Err(Error::from(ErrorKind::InvalidData))
            }
        }
    }

    // decoded subject
    pub fn subject(&self) -> Result<String, Error> {
        match add_der_header(Tag::Sequence, &self.subject) {
            Ok(subject) => match X509Name::from_der(&subject) {
                Ok((_, decoded)) => Ok(decoded.to_string()),
                Err(e) => {
                    log::warn!("failed to decode Subject: {:?}", e);
                    Err(Error::from(ErrorKind::InvalidData))
                }
            },
            Err(e) => {
                log::warn!("failed to add der header to subject: {e}");
                Err(Error::from(ErrorKind::InvalidData))
            }
        }
    }
}

impl fmt::Debug for OwnedTrustAnchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\nDer{:?}", self.subject().unwrap_or("Subject error".to_string()), self.spki)
    }
}

impl fmt::Display for OwnedTrustAnchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.subject().unwrap_or("Subject error".to_string()))
    }
}

impl<'a> From<&TrustAnchor<'a>> for OwnedTrustAnchor {
    fn from(ta: &TrustAnchor) -> Self {
        Self::from_subject_spki_name_constraints(
            ta.subject.as_ref(),
            ta.subject_public_key_info.as_ref(),
            ta.name_constraints.as_ref().map(|nc| nc.as_ref()),
        )
    }
}

impl<'a> Into<TrustAnchor<'a>> for OwnedTrustAnchor {
    fn into(self) -> TrustAnchor<'a> {
        TrustAnchor {
            subject: Der::from(self.subject),
            subject_public_key_info: Der::from(self.spki),
            name_constraints: self.name_constraints.map_or(None, |nc| Some(Der::from(nc))),
        }
    }
}

/// Add a DER header to a DER encoded [u8]
fn add_der_header(tag: Tag, naked: &Vec<u8>) -> Result<Vec<u8>, Error> {
    match Header::new(tag, naked.len()) {
        Ok(header) => {
            let mut buff: [u8; 32] = [0u8; 32];
            match header.encode_to_slice(&mut buff) {
                Ok(der) => Ok([der, naked].concat()),
                Err(_) => Err(Error::new(ErrorKind::InvalidData, "der parse failed: encode")),
            }
        }
        Err(_) => Err(Error::new(ErrorKind::InvalidData, "der parse failed: header")),
    }
}

/// Remove a DER header from a DER encoded [u8]
fn rm_der_header(der: &[u8]) -> Result<Vec<u8>, Error> {
    match der::SliceReader::new(der) {
        Ok(reader) => match reader.peek_header() {
            Ok(header) => match header.encoded_len() {
                Ok(len) => match TryInto::<usize>::try_into(len) {
                    Ok(len) => Ok(der[len..].to_vec()),
                    Err(_) => Err(Error::new(ErrorKind::InvalidData, "der decode failed: into")),
                },
                Err(_) => Err(Error::new(ErrorKind::InvalidData, "der decode failed: length")),
            },
            Err(_) => Err(Error::new(ErrorKind::InvalidData, "der decode failed: header")),
        },
        Err(_) => Err(Error::new(ErrorKind::InvalidData, "der decode failed: reader")),
    }
}
