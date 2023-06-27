// A mirror of rustls::OwnedTrustAnchor

use rkyv::{Archive, Deserialize, Serialize,};
use std::cmp::min;
use std::fmt;
use x509_parser::der_parser::der::Tag;
use x509_parser::prelude::{FromDer, X509Certificate};

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
        RustlsOwnedTrustAnchor {
            subject: x509.subject().as_raw().to_vec(),
            spki: x509.public_key().raw.to_vec(),
            name_constraints: None,
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
