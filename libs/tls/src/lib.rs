use modals::Modals;
use rkyv::{
    de::deserializers::AllocDeserializer,
    ser::{serializers::WriteSerializer, Serializer},
    AlignedVec, Archive, Deserialize, Serialize,
};
use rustls::Certificate;
use std::convert::TryFrom;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::path::PathBuf;
use xous_names::XousNames;

/// PDDB Dict for tls trusted certificates keys
const TLS_CERT_DICT: &str = "tls/cert";
const CURRENT_VERSION_KEY: &str = "__version";

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct RustlsOwnedTrustAnchor {
    subject: Vec<u8>,
    spki: Vec<u8>,
    name_constraints: Option<Vec<u8>>,
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
