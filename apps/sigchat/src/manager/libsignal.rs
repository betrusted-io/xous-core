// this is a stub for a wrapped libsignal



// https://github.com/signalapp/Signal-Android/blob/d2053d2db7b1b930b7058ce5506dd6037ac3b808/libsignal-service/src/main/protowire/Provisioning.proto#L13C9-L15
//
// message ProvisioningUuid {
//   optional string uuid = 1;
// }
pub struct ProvisioningUuid {
    pub id : String,
}
impl ProvisioningUuid {
    pub fn decode(bytes: Vec<u8>) -> ProvisioningUuid {
        log::info!("raw uuid Protocol Buffer: {:?}", bytes);
        ProvisioningUuid {id : "TODO decode uuid Protocol Buffer".to_string()} 
    }
}

//////////////////////////////////////////////////////////

// use org.signal.libsignal.protocol.IdentityKey;
pub struct IdentityKey {
    pub key:String,
}
impl IdentityKey {
    pub fn new(key: String) -> Self {
        IdentityKey {key}
    } 

    pub fn clone(&self) -> IdentityKey {
        IdentityKey::new(self.key.clone())
    }
}

// use org.signal.libsignal.protocol.IdentityKeyPair;
pub struct IdentityKeyPair {
    pub djb_identity_key: IdentityKey,
    pub djb_private_key: IdentityKey,
}


// use org.signal.libsignal.protocol.ecc.Curve;
pub struct Curve {}
impl Curve {
    pub fn generate_key_pair() -> DjbKeyPair {
        DjbKeyPair {
            djb_private_key: IdentityKey::new("STUB privateIdentityKey".to_string()),
            djb_public_key: IdentityKey::new("STUB publicIdentityKey".to_string()),
        }
    }
}

pub struct DjbKeyPair {
    djb_private_key: IdentityKey,
    djb_public_key: IdentityKey,
}
impl DjbKeyPair {
    pub fn get_private_key(&self) -> IdentityKey {
        self.djb_private_key.clone()
    }
    pub fn get_public_key(&self) -> IdentityKey {
        self.djb_public_key.clone()
    }
}


// https://github.com/AsamK/signal-cli/blob/375bdb79485ec90beb9a154112821a4657740b7a/lib/src/main/java/org/asamk/signal/manager/util/KeyUtils.java#L45-L51
pub fn generate_identity_key_pair() -> IdentityKeyPair {
    let djb_key_pair = Curve::generate_key_pair();
    let djb_identity_key = IdentityKey::new(djb_key_pair.get_public_key().key);
    let djb_private_key = djb_key_pair.get_private_key();
    IdentityKeyPair { djb_identity_key, djb_private_key }
}
