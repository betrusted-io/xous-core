mod service_environment;

use crate::manager::libsignal::{DeviceNameUtil, ProvisionMessage, SignalServiceAddress};
use pddb::Pddb;
pub use service_environment::ServiceEnvironment;
use std::io::{Error, ErrorKind, Read, Write};
use std::str::FromStr;
use url::Host;

#[allow(dead_code)]
pub struct Account {
    pddb: Pddb,
    pddb_dict: String,
    aci_identity_private: Option<String>,
    aci_identity_public: Option<String>,
    aci_service_id: Option<String>,
    device_id: u32,
    encrypted_device_name: Option<String>,
    host: Host,
    is_multi_device: bool,
    number: Option<String>,
    password: Option<String>,
    pin_master_key: Option<String>,
    pni_identity_private: Option<String>,
    pni_identity_public: Option<String>,
    pni_service_id: Option<String>,
    profile_key: Option<String>,
    registered: bool,
    service_environment: ServiceEnvironment,
    storage_key: Option<String>,
    store_last_receive_timestamp: i64,
    store_manifest_version: i64,
    store_manifest: Option<String>,
}

pub const DEFAULT_HOST: &str = "signal.org";

const ACI_IDENTITY_PRIVATE_KEY: &str = "aci.identity.private";
const ACI_IDENTITY_PUBLIC_KEY: &str = "aci.identity.public";
const ACI_SERVICE_ID_KEY: &str = "aci.service_id";
const DEVICE_ID_KEY: &str = "device_id";
const ENCRYPTED_DEVICE_NAME_KEY: &str = "encrypted_device_name";
const HOST_KEY: &str = "host";
const IS_MULTI_DEVICE_KEY: &str = "is_multi_device";
const NUMBER_KEY: &str = "number";
const PASSWORD_KEY: &str = "password";
const PIN_MASTER_KEY_KEY: &str = "pin_master_key";
const PNI_IDENTITY_PRIVATE_KEY: &str = "pni.identity.private";
const PNI_IDENTITY_PUBLIC_KEY: &str = "pni.identity.public";
const PNI_SERVICE_ID_KEY: &str = "pni.service_id";
const PROFILE_KEY_KEY: &str = "profile_key";
const REGISTERED_KEY: &str = "registered";
const SERVICE_ENVIRONMENT_KEY: &str = "service_environment";
const STORAGE_KEY_KEY: &str = "storage_key";
const STORE_LAST_RECEIVE_TIMESTAMP_KEY: &str = "store_last_receive_timestamp";
const STORE_MANIFEST_VERSION_KEY: &str = "store_manifest_version";
const STORE_MANIFEST_KEY: &str = "store_manifest";

impl Account {
    /// Create a new Account stored in pddb with default values
    ///
    /// This function saves default values for each field in the pddb
    /// and then calls read() to load the values into the Account struct
    ///
    /// # Arguments
    /// * `pddb_dict` - pddb dictionary name to hold the Account
    ///
    /// # Returns
    ///
    /// a new Account with default values
    ///
    pub fn new(
        pddb_dict: &str,
        host: &Host,
        service_environment: &ServiceEnvironment,
    ) -> Result<Account, Error> {
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        set(&pddb, pddb_dict, ACI_IDENTITY_PRIVATE_KEY, None)?;
        set(&pddb, pddb_dict, ACI_IDENTITY_PUBLIC_KEY, None)?;
        set(&pddb, pddb_dict, ACI_SERVICE_ID_KEY, None)?;
        set(&pddb, pddb_dict, DEVICE_ID_KEY, Some("0"))?;
        set(&pddb, pddb_dict, ENCRYPTED_DEVICE_NAME_KEY, None)?;
        set(&pddb, pddb_dict, HOST_KEY, Some(&host.to_string()))?;
        set(
            &pddb,
            pddb_dict,
            IS_MULTI_DEVICE_KEY,
            Some(&false.to_string()),
        )?;
        set(&pddb, pddb_dict, NUMBER_KEY, None)?;
        set(&pddb, pddb_dict, PASSWORD_KEY, None)?;
        set(&pddb, pddb_dict, PIN_MASTER_KEY_KEY, None)?;
        set(&pddb, pddb_dict, PNI_IDENTITY_PRIVATE_KEY, None)?;
        set(&pddb, pddb_dict, PNI_IDENTITY_PUBLIC_KEY, None)?;
        set(&pddb, pddb_dict, PNI_SERVICE_ID_KEY, None)?;
        set(&pddb, pddb_dict, PROFILE_KEY_KEY, None)?;
        set(&pddb, pddb_dict, REGISTERED_KEY, Some(&false.to_string()))?;
        set(
            &pddb,
            pddb_dict,
            SERVICE_ENVIRONMENT_KEY,
            Some(&service_environment.to_string()),
        )?;
        set(&pddb, pddb_dict, STORAGE_KEY_KEY, None)?;
        set(
            &pddb,
            pddb_dict,
            STORE_LAST_RECEIVE_TIMESTAMP_KEY,
            Some("0"),
        )?;
        set(&pddb, pddb_dict, STORE_MANIFEST_VERSION_KEY, Some("-1"))?;
        set(&pddb, pddb_dict, STORE_MANIFEST_KEY, None)?;
        Account::read(pddb_dict)
    }

    // retrieves an existing Account from the pddb
    //
    // # Arguments
    // * `pddb_dict` - the pddb dictionary name holding the Account
    //
    // # Returns
    //
    // a Account with values read from pddb_dict
    //
    pub fn read(pddb_dict: &str) -> Result<Account, Error> {
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        match (
            get(&pddb, pddb_dict, ACI_IDENTITY_PRIVATE_KEY),
            get(&pddb, pddb_dict, ACI_IDENTITY_PUBLIC_KEY),
            get(&pddb, pddb_dict, ACI_SERVICE_ID_KEY),
            get(&pddb, pddb_dict, DEVICE_ID_KEY),
            get(&pddb, pddb_dict, ENCRYPTED_DEVICE_NAME_KEY),
            get(&pddb, pddb_dict, HOST_KEY),
            get(&pddb, pddb_dict, IS_MULTI_DEVICE_KEY),
            get(&pddb, pddb_dict, NUMBER_KEY),
            get(&pddb, pddb_dict, PASSWORD_KEY),
            get(&pddb, pddb_dict, PIN_MASTER_KEY_KEY),
            get(&pddb, pddb_dict, PNI_IDENTITY_PRIVATE_KEY),
            get(&pddb, pddb_dict, PNI_IDENTITY_PUBLIC_KEY),
            get(&pddb, pddb_dict, PNI_SERVICE_ID_KEY),
            get(&pddb, pddb_dict, PROFILE_KEY_KEY),
            get(&pddb, pddb_dict, REGISTERED_KEY),
            get(&pddb, pddb_dict, SERVICE_ENVIRONMENT_KEY),
            get(&pddb, pddb_dict, STORAGE_KEY_KEY),
            get(&pddb, pddb_dict, STORE_LAST_RECEIVE_TIMESTAMP_KEY),
            get(&pddb, pddb_dict, STORE_MANIFEST_VERSION_KEY),
            get(&pddb, pddb_dict, STORE_MANIFEST_KEY),
        ) {
            (
                Ok(aci_identity_private),
                Ok(aci_identity_public),
                Ok(aci_service_id),
                Ok(Some(device_id)),
                Ok(encrypted_device_name),
                Ok(Some(host)),
                Ok(Some(is_multi_device)),
                Ok(number),
                Ok(password),
                Ok(pin_master_key),
                Ok(pni_identity_private),
                Ok(pni_identity_public),
                Ok(pni_service_id),
                Ok(profile_key),
                Ok(Some(registered)),
                Ok(Some(service_environment)),
                Ok(storage_key),
                Ok(Some(store_last_receive_timestamp)),
                Ok(Some(store_manifest_version)),
                Ok(store_manifest),
            ) => Ok(Account {
                pddb: pddb,
                pddb_dict: pddb_dict.to_string(),
                aci_identity_private: aci_identity_private,
                aci_identity_public: aci_identity_public,
                aci_service_id: aci_service_id,
                device_id: device_id.parse().unwrap(),
                encrypted_device_name: encrypted_device_name,
                host: Host::parse(&host).unwrap(),
                is_multi_device: is_multi_device.parse().unwrap(),
                number: number,
                password: password,
                pin_master_key: pin_master_key,
                pni_identity_private: pni_identity_private,
                pni_identity_public: pni_identity_public,
                pni_service_id: pni_service_id,
                profile_key: profile_key,
                registered: registered.parse().unwrap(),
                service_environment: ServiceEnvironment::from_str(&service_environment).unwrap(),
                storage_key: storage_key,
                store_last_receive_timestamp: store_last_receive_timestamp.parse().unwrap(),
                store_manifest_version: store_manifest_version.parse().unwrap(),
                store_manifest: store_manifest,
            }),
            (Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _, _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e), _) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, Err(e)) => Err(e),
            (_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _) => {
                Err(Error::from(ErrorKind::InvalidData))
            }
        }
    }

    /// Delete this Account key/value from the pddb
    ///
    /// While this Account struct will persist in memory, a subsequent Account.read() will fail
    ///
    pub fn delete(pddb_dict: &str) -> Result<(), Error> {
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        pddb.delete_dict(pddb_dict, None)?;
        log::info!("deleted Signal Account from pddb");
        Ok(())
    }

    /// link to an existing Signal Account as a secondary device
    ///
    /// Confirm that the state of the Signal Account is OK before linking
    /// https://github.com/AsamK/signal-cli/blob/375bdb79485ec90beb9a154112821a4657740b7a/lib/src/main/java/org/asamk/signal/manager/internal/ProvisioningManagerImpl.java#L200-L239
    ///
    /// # Arguments
    ///
    /// * `device_name` - name to describe this new device
    /// * `provisioning_msg` - obtained from the Signal server
    ///
    /// # Returns
    ///
    /// true on success
    ///
    pub fn link(
        &mut self,
        device_name: &str,
        provisioning_msg: ProvisionMessage,
    ) -> Result<bool, Error> {
        // Check if this device can be relinked
        if self.is_primary_device() {
            log::warn!("failed to link device as already registered as primary");
            return Ok(false);
        }
        // TODO complete link pre-checks
        // } else if self.is_registered()
        // && self.service_environment() != provisioning_msg.service_environment{
        //     log::warn!("failed to link device as already registered in different Service Environment");
        //     return Ok(false);
        // }

        let aci = provisioning_msg.aci;
        self.set(ACI_IDENTITY_PRIVATE_KEY, Some(&aci.djb_private_key.key))?;
        self.set(ACI_IDENTITY_PUBLIC_KEY, Some(&aci.djb_identity_key.key))?;
        self.set(ACI_SERVICE_ID_KEY, Some(&aci.service_id))?;
        self.set(DEVICE_ID_KEY, Some("0"))?;
        self.set(
            ENCRYPTED_DEVICE_NAME_KEY,
            Some(&DeviceNameUtil::encrypt_device_name(
                device_name,
                aci.djb_private_key,
            )),
        )?;
        self.set(IS_MULTI_DEVICE_KEY, Some(&true.to_string()))?;
        self.set(NUMBER_KEY, Some(&provisioning_msg.number))?;
        self.set(PASSWORD_KEY, Some("STUB 32 bytes"))?;
        self.set(PIN_MASTER_KEY_KEY, Some(&provisioning_msg.master_key))?;
        let pni = provisioning_msg.pni;
        self.set(PNI_IDENTITY_PRIVATE_KEY, Some(&pni.djb_private_key.key))?;
        self.set(PNI_IDENTITY_PUBLIC_KEY, Some(&pni.djb_identity_key.key))?;
        self.set(PNI_SERVICE_ID_KEY, Some(&aci.service_id))?;
        self.set(
            PROFILE_KEY_KEY,
            Some(
                &provisioning_msg
                    .profile_key
                    .unwrap_or_else(|| "STUB 32 bytes".to_string()),
            ),
        )?;
        self.set(REGISTERED_KEY, Some(&false.to_string()))?;
        self.set(STORAGE_KEY_KEY, None)?;
        self.set(STORE_LAST_RECEIVE_TIMESTAMP_KEY, Some("0"))?;
        self.set(STORE_MANIFEST_VERSION_KEY, Some("-1"))?;
        self.set(STORE_MANIFEST_KEY, None)?;

        // TODO complete registration setup
        // https://github.com/AsamK/signal-cli/blob/375bdb79485ec90beb9a154112821a4657740b7a/lib/src/main/java/org/asamk/signal/manager/storage/SignalAccount.java#L270-L306
        // getRecipientTrustedResolver().resolveSelfRecipientTrusted(getSelfRecipientAddress());
        // getSenderKeyStore().deleteAll();
        // trustSelfIdentity(ServiceIdType.ACI);
        // trustSelfIdentity(ServiceIdType.PNI);
        // aciAccountData.getSessionStore().archiveAllSessions();
        // pniAccountData.getSessionStore().archiveAllSessions();
        // clearAllPreKeys();
        // getKeyValueStore().storeEntry(lastRecipientsRefresh, null);

        Ok(true)
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn is_primary_device(&self) -> bool {
        self.device_id == SignalServiceAddress::DEFAULT_DEVICE_ID
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    #[allow(dead_code)]
    pub fn number(&self) -> Option<&str> {
        match &self.number {
            Some(num) => Some(&num),
            None => None,
        }
    }

    pub fn service_environment(&self) -> &ServiceEnvironment {
        &self.service_environment
    }

    #[allow(dead_code)]
    pub fn set_number(&mut self, value: &str) -> Result<(), Error> {
        match self.set(NUMBER_KEY, Some(value)) {
            Ok(_) => self.number = Some(value.to_string()),
            Err(e) => log::warn!("failed to set signal account number: {e}"),
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn get(&self, key: &str) -> Result<Option<String>, Error> {
        get(&self.pddb, &self.pddb_dict, key)
    }

    // Sets the value of a pddb_key / field in the Account
    //
    // To guarantee consistency, the value is saved to the pddb and,
    // on success, set to the corresponding field in the Account struct.
    //
    // # Arguments
    // * `key` - the pddb_key corresponding to the Account field
    // * `value` - the value to save into the Account field (and pddb)
    //
    // # Returns
    //
    // Ok()
    //
    fn set(&mut self, key: &str, value: Option<&str>) -> Result<(), Error> {
        let owned_value = value.map(str::to_string);
        match set(&self.pddb, &self.pddb_dict, key, value) {
            Ok(()) => match key {
                ACI_IDENTITY_PRIVATE_KEY => Ok(self.aci_identity_private = owned_value),
                ACI_IDENTITY_PUBLIC_KEY => Ok(self.aci_identity_public = owned_value),
                ACI_SERVICE_ID_KEY => Ok(self.aci_service_id = owned_value),
                DEVICE_ID_KEY => Ok(self.device_id = owned_value.unwrap().parse().unwrap()),
                IS_MULTI_DEVICE_KEY => {
                    Ok(self.is_multi_device = owned_value.unwrap().parse().unwrap())
                }
                NUMBER_KEY => Ok(self.number = owned_value),
                PASSWORD_KEY => Ok(self.password = owned_value),
                PIN_MASTER_KEY_KEY => Ok(self.pin_master_key = owned_value),
                PNI_IDENTITY_PRIVATE_KEY => Ok(self.pni_identity_private = owned_value),
                PNI_IDENTITY_PUBLIC_KEY => Ok(self.pni_identity_public = owned_value),
                PNI_SERVICE_ID_KEY => Ok(self.pni_service_id = owned_value),
                PROFILE_KEY_KEY => Ok(self.profile_key = owned_value),
                REGISTERED_KEY => Ok(self.registered = owned_value.unwrap().parse().unwrap()),
                SERVICE_ENVIRONMENT_KEY => Ok(self.service_environment =
                    ServiceEnvironment::from_str(&value.unwrap()).unwrap()),
                STORAGE_KEY_KEY => Ok(self.storage_key = owned_value),
                _ => {
                    log::warn!("invalid key: {key}");
                    let _ = &self.pddb.delete_key(&self.pddb_dict, &key, None);
                    Err(Error::from(ErrorKind::NotFound))
                }
            },
            Err(e) => Err(e),
        }
    }
}

fn get(pddb: &Pddb, dict: &str, key: &str) -> Result<Option<String>, Error> {
    let value = match pddb.get(dict, key, None, true, false, None, None::<fn()>) {
        Ok(mut pddb_key) => {
            let mut buffer = [0; 256];
            match pddb_key.read(&mut buffer) {
                Ok(len) => match String::from_utf8(buffer[..len].to_vec()) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::warn!("failed to String: {:?}", e);
                        None
                    }
                },
                Err(e) => {
                    log::warn!("failed pddb_key read: {:?}", e);
                    None
                }
            }
        }
        Err(_) => None,
    };
    log::info!("get '{}' = '{:?}'", key, value);
    Ok(value)
}

fn set(pddb: &Pddb, dict: &str, key: &str, value: Option<&str>) -> Result<(), Error> {
    log::info!("set '{}' = '{:?}'", key, value);
    // delete key first to ensure data in a prior longer key is gone
    pddb.delete_key(dict, key, None).ok();
    if let Some(value) = value {
        match pddb.get(dict, key, None, true, true, None, None::<fn()>) {
            Ok(mut pddb_key) => match pddb_key.write(&value.as_bytes()) {
                Ok(len) => {
                    pddb.sync().ok();
                    log::trace!("Wrote {} bytes to {}:{}", len, dict, key);
                }
                Err(e) => {
                    log::warn!("Error writing {}:{} {:?}", dict, key, e);
                }
            },
            Err(e) => log::warn!("failed to set pddb {}:{}  {:?}", dict, key, e),
        };
    }
    Ok(())
}
