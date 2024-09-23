#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod backups;
mod sha512_digest;

use core::ops::Deref;
use std::convert::TryInto;
use std::format;
use std::str;

#[cfg(feature = "policy-menu")]
use String;
use gam::modal::*;
#[cfg(feature = "policy-menu")]
use gam::{MenuItem, MenuPayload};
use locales::t;
use num_traits::*;
#[cfg(feature = "tts")]
use tts_frontend::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message};
use xous_ipc::Buffer;

#[cfg(any(feature = "precursor", feature = "renode"))]
mod implementation;
#[cfg(any(feature = "precursor", feature = "renode"))]
use implementation::*;
/// used by the bbram helper/console protocol to indicate the start of a console message
const CONSOLE_SENTINEL: &'static str = "CONS_SENTINEL|";

#[cfg(any(feature = "precursor", feature = "renode"))]
mod bcrypt;

pub enum SignatureResult {
    SelfSignOk,
    ThirdPartyOk,
    DevKeyOk,
    Invalid,
    MalformedSignature,
    InvalidSignatureType,
    InvalidPubKey,
}
#[allow(dead_code)]
pub enum GatewareRegion {
    Boot,
    Staging,
}

#[derive(Eq, PartialEq)]
pub(crate) enum UpdateType {
    Regular,
    BbramProvision,
    Restore,
    #[allow(dead_code)]
    EfuseProvision,
}

/// An "easily" parseable metadata structure in flash. There's nothing that guarantees the authenticity
/// of the metadata in and of itself, other than the digital signature that wraps the entire gateware record.
/// Thus we're relying on the person who signs the gateware to not inject false data here.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct MetadataInFlash {
    pub magic: u32, // 0x6174656d 'atem'
    pub version: u32,
    /// git data, but formatted as binary integers
    pub git_additional: u32, // commits beyond the tag
    pub git_rev: u32,
    pub git_min: u32,
    pub git_maj: u32,
    pub git_commit: u32,
    /// md5sum of the dummy-encrypted source file; not meant to be secure, just for human-ID purposes
    pub bin_checksum: [u8; 16],
    /// md5sum of 'betrusted_soc.py'
    pub src_checksum: [u8; 16],
    /// date as free-form string (for human readable purposes)
    pub date_len: u32,
    pub date_str: [u8; 64],
    /// the host on which the image was built
    pub host_len: u32,
    pub host_str: [u8; 64],
    /// git tag info as a free-form string
    pub tag_len: u32,
    pub tag_str: [u8; 64],
    /// git log info of the last commit, as a free-form string.
    pub log_len: u32,
    pub log_str: [u8; 512],
    /// status of the build tree, as a free-form string.
    pub status_len: u32,
    pub status_str: [u8; 1024],
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use std::convert::TryInto;

    use aes::Aes256;
    use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
    use ed25519_dalek::VerifyingKey;
    use gam::modal::{Modal, Slider};
    use gam::{ActionType, ProgressBar};
    use locales::t;
    use num_traits::*;
    use xous_semver::SemVer;

    use crate::PasswordRetentionPolicy;
    use crate::PasswordType;
    use crate::UpdateType;
    use crate::api::*;
    use crate::backups;
    use crate::{GatewareRegion, MetadataInFlash, SignatureResult};

    #[derive(Debug, Copy, Clone)]
    #[allow(dead_code)]
    pub enum FpgaKeySource {
        Bbram,
        Efuse,
    }
    #[allow(dead_code)]
    pub(crate) struct RootKeys {
        password_type: Option<PasswordType>,
        jtag: jtag::Jtag,
        xns: xous_names::XousNames,
        ticktimer: ticktimer_server::Ticktimer,
        gam: gam::Gam,
    }

    #[allow(dead_code)]
    impl RootKeys {
        pub fn new() -> RootKeys {
            let xns = xous_names::XousNames::new().unwrap();
            let jtag = jtag::Jtag::new(&xns).expect("couldn't connect to jtag server");
            RootKeys {
                password_type: None,
                // must occupy this connection for the system to boot properly
                jtag,
                ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
                gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
                xns,
            }
        }

        pub fn suspend(&self) {}

        pub fn resume(&self) {}

        pub fn pddb_recycle(&self) {}

        pub fn update_policy(&mut self, policy: Option<PasswordRetentionPolicy>) {
            log::info!("policy updated: {:?}", policy);
        }

        pub fn hash_and_save_password(&mut self, pw: &str, _verify: bool) -> bool {
            log::info!("got password plaintext: {}", pw);
            true
        }

        pub fn set_ux_password_type(&mut self, cur_type: Option<PasswordType>) {
            self.password_type = cur_type;
        }

        pub fn is_initialized(&self) -> bool { true }

        pub fn setup_key_init(&mut self) {}

        fn fake_progress(
            &mut self,
            rootkeys_modal: &mut Modal,
            main_cid: xous::CID,
            msg: &str,
        ) -> Result<(), RootkeyResult> {
            let mut progress_action = Slider::new(
                main_cid,
                Opcode::UxGutter.to_u32().unwrap(),
                0,
                100,
                10,
                Some("%"),
                0,
                true,
                true,
            );
            progress_action.set_is_password(true);
            // now show the init wait note...
            rootkeys_modal.modify(
                Some(ActionType::Slider(progress_action)),
                Some(msg),
                false,
                None,
                true,
                None,
            );
            rootkeys_modal.activate();

            xous::yield_slice(); // give some time to the GAM to render
            // capture the progress bar elements in a convenience structure
            let mut pb = ProgressBar::new(rootkeys_modal, &mut progress_action);

            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
            for i in 1..10 {
                log::info!("fake progress: {}", i * 10);
                pb.set_percentage(i * 10);
                ticktimer.sleep_ms(2000).unwrap();
            }
            Ok(())
        }

        pub fn do_key_init(
            &mut self,
            rootkeys_modal: &mut Modal,
            main_cid: xous::CID,
        ) -> Result<(), RootkeyResult> {
            self.xous_init_interlock();
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.setup_wait", locales::LANG))
        }

        pub fn do_gateware_update(
            &mut self,
            rootkeys_modal: &mut Modal,
            _modals: &modals::Modals,
            main_cid: xous::CID,
            _updatetype: UpdateType,
        ) -> Result<(), RootkeyResult> {
            self.xous_init_interlock();
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.gwup_starting", locales::LANG))
        }

        pub fn do_gateware_provision_uninitialized(
            &mut self,
            rootkeys_modal: &mut Modal,
            main_cid: xous::CID,
        ) -> Result<(), RootkeyResult> {
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.gwup_starting", locales::LANG))
        }

        pub fn do_sign_xous(
            &mut self,
            rootkeys_modal: &mut Modal,
            main_cid: xous::CID,
        ) -> Result<(), RootkeyResult> {
            self.xous_init_interlock();
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.init.signing_kernel", locales::LANG))
        }

        pub fn purge_password(&mut self, _ptype: PasswordType) {}

        pub fn purge_user_password(&mut self, _ptype: AesRootkeyType) {}

        pub fn purge_sensitive_data(&mut self) {}

        pub fn get_ux_password_type(&self) -> Option<PasswordType> { self.password_type }

        pub fn finish_key_init(&mut self) {}

        pub fn verify_gateware_self_signature(&mut self, _pk: Option<&VerifyingKey>) -> bool { true }

        pub fn test(
            &mut self,
            _rootkeys_modal: &mut Modal,
            _main_cid: xous::CID,
        ) -> Result<(), RootkeyResult> {
            Ok(())
        }

        pub fn is_jtag_working(&self) -> bool { true }

        pub fn is_efuse_secured(&self) -> Option<bool> { None }

        pub fn check_gateware_signature(&mut self, _region_enum: GatewareRegion) -> SignatureResult {
            log::info!("faking gateware check...");
            self.ticktimer.sleep_ms(4000).unwrap();
            log::info!("done");
            SignatureResult::DevKeyOk
        }

        pub fn is_pcache_update_password_valid(&self) -> bool { false }

        pub fn is_pcache_boot_password_valid(&self) -> bool {
            // set this to `false` to test password boxes in hosted mode
            true
        }

        pub fn fpga_key_source(&self) -> FpgaKeySource { FpgaKeySource::Efuse }

        pub fn fetch_gw_metadata(&self, _region_enum: GatewareRegion) -> MetadataInFlash {
            MetadataInFlash {
                magic: 0x6174656d,
                version: 1,
                git_additional: 27,
                git_rev: 2,
                git_min: 8,
                git_maj: 0,
                git_commit: 0x12345678,
                bin_checksum: [0; 16],
                src_checksum: [0; 16],
                date_len: 26,
                date_str: [
                    50, 48, 50, 49, 45, 48, 56, 45, 49, 50, 32, 50, 50, 58, 49, 53, 58, 53, 51, 46, 56, 49,
                    55, 51, 53, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                host_len: 14,
                host_str: [
                    98, 117, 110, 110, 105, 101, 45, 100, 101, 115, 107, 116, 111, 112, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                tag_len: 19,
                tag_str: [
                    118, 48, 46, 56, 46, 50, 45, 55, 49, 45, 103, 102, 102, 98, 97, 52, 55, 102, 10, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                log_len: 203,
                log_str: [
                    99, 111, 109, 109, 105, 116, 32, 102, 102, 98, 97, 52, 55, 102, 52, 98, 102, 55, 99, 52,
                    51, 50, 55, 54, 55, 50, 50, 56, 102, 101, 99, 52, 51, 53, 97, 56, 56, 48, 54, 54, 55, 53,
                    101, 52, 102, 49, 102, 10, 65, 117, 116, 104, 111, 114, 58, 32, 98, 117, 110, 110, 105,
                    101, 32, 60, 98, 117, 110, 110, 105, 101, 64, 107, 111, 115, 97, 103, 105, 46, 99, 111,
                    109, 62, 10, 68, 97, 116, 101, 58, 32, 32, 32, 84, 104, 117, 32, 65, 117, 103, 32, 49,
                    50, 32, 48, 52, 58, 52, 49, 58, 53, 49, 32, 50, 48, 50, 49, 32, 43, 48, 56, 48, 48, 10,
                    10, 32, 32, 32, 32, 109, 111, 100, 105, 102, 121, 32, 98, 111, 111, 116, 32, 116, 111,
                    32, 100, 111, 32, 102, 97, 108, 108, 98, 97, 99, 107, 32, 111, 110, 32, 115, 105, 103,
                    110, 97, 116, 117, 114, 101, 115, 10, 10, 77, 9, 98, 111, 111, 116, 47, 98, 101, 116,
                    114, 117, 115, 116, 101, 100, 45, 98, 111, 111, 116, 47, 115, 114, 99, 47, 109, 97, 105,
                    110, 46, 114, 115, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                status_len: 512,
                status_str: [
                    79, 110, 32, 98, 114, 97, 110, 99, 104, 32, 109, 97, 105, 110, 10, 89, 111, 117, 114, 32,
                    98, 114, 97, 110, 99, 104, 32, 105, 115, 32, 117, 112, 32, 116, 111, 32, 100, 97, 116,
                    101, 32, 119, 105, 116, 104, 32, 39, 111, 114, 105, 103, 105, 110, 47, 109, 97, 105, 110,
                    39, 46, 10, 10, 67, 104, 97, 110, 103, 101, 115, 32, 110, 111, 116, 32, 115, 116, 97,
                    103, 101, 100, 32, 102, 111, 114, 32, 99, 111, 109, 109, 105, 116, 58, 10, 32, 32, 40,
                    117, 115, 101, 32, 34, 103, 105, 116, 32, 97, 100, 100, 32, 60, 102, 105, 108, 101, 62,
                    46, 46, 46, 34, 32, 116, 111, 32, 117, 112, 100, 97, 116, 101, 32, 119, 104, 97, 116, 32,
                    119, 105, 108, 108, 32, 98, 101, 32, 99, 111, 109, 109, 105, 116, 116, 101, 100, 41, 10,
                    32, 32, 40, 117, 115, 101, 32, 34, 103, 105, 116, 32, 114, 101, 115, 116, 111, 114, 101,
                    32, 60, 102, 105, 108, 101, 62, 46, 46, 46, 34, 32, 116, 111, 32, 100, 105, 115, 99, 97,
                    114, 100, 32, 99, 104, 97, 110, 103, 101, 115, 32, 105, 110, 32, 119, 111, 114, 107, 105,
                    110, 103, 32, 100, 105, 114, 101, 99, 116, 111, 114, 121, 41, 10, 32, 32, 40, 99, 111,
                    109, 109, 105, 116, 32, 111, 114, 32, 100, 105, 115, 99, 97, 114, 100, 32, 116, 104, 101,
                    32, 117, 110, 116, 114, 97, 99, 107, 101, 100, 32, 111, 114, 32, 109, 111, 100, 105, 102,
                    105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 32, 105, 110, 32, 115, 117, 98, 109,
                    111, 100, 117, 108, 101, 115, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32,
                    32, 32, 97, 112, 112, 101, 110, 100, 95, 99, 115, 114, 46, 112, 121, 10, 9, 109, 111,
                    100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 101, 116, 114, 117, 115, 116, 101, 100,
                    95, 115, 111, 99, 46, 112, 121, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32,
                    32, 32, 98, 111, 111, 116, 47, 98, 101, 116, 114, 117, 115, 116, 101, 100, 45, 98, 111,
                    111, 116, 47, 97, 115, 115, 101, 109, 98, 108, 101, 46, 115, 104, 10, 9, 109, 111, 100,
                    105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 111, 111, 116, 95, 116, 101, 115, 116, 46,
                    112, 121, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 117, 105,
                    108, 100, 45, 100, 111, 99, 115, 46, 115, 104, 10, 9, 109, 111, 100, 105, 102, 105, 101,
                    100, 58, 32, 32, 32, 99, 104, 101, 99, 107, 45, 116, 105, 109, 105, 110, 103, 46, 115,
                    104, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115,
                    47, 99, 111, 109, 112, 105, 108, 101, 114, 95, 114, 116, 32, 40, 109, 111, 100, 105, 102,
                    105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102,
                    105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 101, 110, 99, 114, 121, 112, 116,
                    45, 98, 105, 116, 115, 116, 114, 101, 97, 109, 45, 112, 121, 116, 104, 111, 110, 32, 40,
                    109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9,
                    109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 103, 97,
                    116, 101, 119, 97, 114, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111,
                    110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32,
                    32, 100, 101, 112, 115, 47, 108, 105, 116, 101, 100, 114, 97, 109, 32, 40, 109, 111, 100,
                    105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100,
                    105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 108, 105, 116, 101, 115,
                    99, 111, 112, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116,
                    101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100,
                    101, 112, 115, 47, 108, 105, 116, 101, 120, 32, 40, 109, 111, 100, 105, 102, 105, 101,
                    100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101,
                    100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 109, 105, 103, 101, 110, 32, 40, 109, 111,
                    100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111,
                    100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 111, 112, 101, 110,
                    116, 105, 116, 97, 110, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110,
                    116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32,
                    100, 101, 112, 115, 47, 112, 121, 115, 101, 114, 105, 97, 108, 32, 40, 109, 111, 100,
                    105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100,
                    105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 112, 121, 116, 104, 111,
                    110, 100, 97, 116, 97, 45, 99, 112, 117, 45, 118, 101, 120, 114, 105, 115, 99, 118, 32,
                    40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10,
                    9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 114,
                    111, 109, 45, 108, 111, 99, 97, 116, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100,
                    32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102,
                ],
            }
        }

        pub fn aes_op(&mut self, key_index: u8, op_type: AesOpType, block: &mut [u8; 16]) {
            // fake a "well known" key by just expanding the index into a trivial key
            let mut key = [0 as u8; 32];
            key[0] = key_index;
            let cipher = Aes256::new(GenericArray::from_slice(&key));
            match op_type {
                AesOpType::Decrypt => cipher.decrypt_block(block.try_into().unwrap()),
                AesOpType::Encrypt => cipher.encrypt_block(block.try_into().unwrap()),
            }
        }

        pub fn aes_par_op(&mut self, key_index: u8, op_type: AesOpType, blocks: &mut [[u8; 16]; PAR_BLOCKS]) {
            // fake a "well known" key by just expanding the index into a trivial key
            let mut key = [0 as u8; 32];
            key[0] = key_index;
            let cipher = Aes256::new(GenericArray::from_slice(&key));
            match op_type {
                AesOpType::Decrypt => {
                    for block in blocks.iter_mut() {
                        cipher.decrypt_block(block.try_into().unwrap());
                    }
                }
                AesOpType::Encrypt => {
                    for block in blocks.iter_mut() {
                        cipher.encrypt_block(block.try_into().unwrap());
                    }
                }
            }
        }

        pub fn kwp_op(&mut self, kwp: &mut KeyWrapper) {
            use aes_kw::Kek;
            use aes_kw::KekAes256;
            let keywrapper: KekAes256 = Kek::from([0u8; 32]);
            match kwp.op {
                KeyWrapOp::Wrap => {
                    match keywrapper.wrap_with_padding_vec(&kwp.data[..kwp.len as usize]) {
                        Ok(wrapped) => {
                            for (&src, dst) in wrapped.iter().zip(kwp.data.iter_mut()) {
                                *dst = src;
                            }
                            kwp.len = wrapped.len() as u32;
                            kwp.result = None;
                            // this is an un-used field but...why not?
                            kwp.expected_len = wrapped.len() as u32;
                        }
                        Err(e) => {
                            kwp.result = Some(match e {
                                aes_kw::Error::IntegrityCheckFailed => KeywrapError::IntegrityCheckFailed,
                                aes_kw::Error::InvalidDataSize => KeywrapError::InvalidDataSize,
                                aes_kw::Error::InvalidKekSize { size } => {
                                    log::info!("invalid size {}", size); // weird. can't name this _size
                                    KeywrapError::InvalidKekSize
                                }
                                aes_kw::Error::InvalidOutputSize { expected } => {
                                    log::info!("invalid output size {}", expected);
                                    KeywrapError::InvalidOutputSize
                                }
                            });
                        }
                    }
                }
                KeyWrapOp::Unwrap => {
                    match keywrapper.unwrap_with_padding_vec(&kwp.data[..kwp.len as usize]) {
                        Ok(wrapped) => {
                            for (&src, dst) in wrapped.iter().zip(kwp.data.iter_mut()) {
                                *dst = src;
                            }
                            kwp.len = wrapped.len() as u32;
                            kwp.result = None;
                            // this is an un-used field but...why not?
                            kwp.expected_len = wrapped.len() as u32;
                        }
                        Err(e) => {
                            kwp.result = Some(match e {
                                aes_kw::Error::IntegrityCheckFailed => KeywrapError::IntegrityCheckFailed,
                                aes_kw::Error::InvalidDataSize => KeywrapError::InvalidDataSize,
                                aes_kw::Error::InvalidKekSize { size } => {
                                    log::info!("invalid size {}", size); // weird. can't name this _size
                                    KeywrapError::InvalidKekSize
                                }
                                aes_kw::Error::InvalidOutputSize { expected } => {
                                    log::info!("invalid output size {}", expected);
                                    KeywrapError::InvalidOutputSize
                                }
                            });
                        }
                    }
                }
            }
        }

        fn xous_init_interlock(&self) {
            loop {
                if self.xns.trusted_init_done().expect("couldn't query init done status on xous-names") {
                    break;
                } else {
                    log::warn!(
                        "trusted init of xous-names not finished, rootkeys is holding off on sensitive operations"
                    );
                    self.ticktimer.sleep_ms(650).expect("couldn't sleep");
                }
            }
            loop {
                if self.gam.trusted_init_done().expect("couldn't query init done status on GAM") {
                    break;
                } else {
                    log::warn!(
                        "trusted init of GAM not finished, rootkeys is holding off on sensitive operations"
                    );
                    self.ticktimer.sleep_ms(650).expect("couldn't sleep");
                }
            }
        }

        pub fn staged_semver(&self) -> SemVer { SemVer { maj: 0, min: 0, rev: 0, extra: 0, commit: None } }

        pub fn try_nokey_soc_update(&mut self, _rootkeys_modal: &mut Modal, _main_cid: xous::CID) -> bool {
            false
        }

        pub fn should_prompt_for_update(&self) -> bool { true }

        pub fn set_prompt_for_update(&self, _state: bool) {}

        pub fn write_backup(
            &mut self,
            mut header: BackupHeader,
            backup_ct: backups::BackupDataCt,
            checksums: Option<Checksums>,
        ) -> Result<(), xous::Error> {
            header.op = BackupOp::Backup;
            log::info!("backup header: {:?}", header);
            log::info!("backup ciphertext: {:x?}", backup_ct.as_ref());
            log::info!("backup checksums: {:x?}", checksums);
            Ok(())
        }

        pub fn write_restore_dna(
            &mut self,
            mut header: BackupHeader,
            backup_ct: backups::BackupDataCt,
        ) -> Result<(), xous::Error> {
            header.op = BackupOp::RestoreDna;
            log::info!("write restore_dna called");
            log::info!("backup header: {:?}", header);
            log::info!("backup ciphertext: {:x?}", backup_ct.as_ref());
            Ok(())
        }

        pub fn read_backup(&mut self) -> Result<(BackupHeader, backups::BackupDataCt), xous::Error> {
            Err(xous::Error::InternalError)
        }

        pub fn erase_backup(&mut self) {}

        pub fn read_backup_header(&mut self) -> Option<BackupHeader> { None }

        pub fn get_backup_key(&mut self) -> Option<(backups::BackupKey, backups::KeyRomExport)> { None }

        pub fn is_zero_key(&self) -> Option<bool> { Some(true) }

        pub fn setup_restore_init(&mut self, _key: backups::BackupKey, _rom: backups::KeyRomExport) {}

        pub fn is_dont_ask_init_set(&self) -> bool { false }

        pub fn set_dont_ask_init(&self) {}

        pub fn reset_dont_ask_init(&mut self) {}
    }
}

fn main() -> ! {
    #[cfg(not(target_os = "xous"))]
    use crate::implementation::RootKeys;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
       Connections allowed to the keys server:
          0. Password entry UX thread (self, created without xns)
          0. Key purge timer (self, created without xns)
          1. Shellchat for test initiation
          2. Main menu -> trigger initialization
          3. PDDB
    */
    let keys_sid = xns.register_name(api::SERVER_NAME_KEYS, Some(3)).expect("can't register server");

    let mut keys = RootKeys::new();
    log::info!("Boot FPGA key source: {:?}", keys.fpga_key_source());

    // create the servers necessary to coordinate an auto-reboot sequence...
    let llio = llio::Llio::new(&xns);
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let com = com::Com::new(&xns).unwrap();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let main_cid = xous::connect(keys_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(None, &xns, api::Opcode::SuspendResume as u32, main_cid)
        .expect("couldn't create suspend/resume object");

    #[cfg(feature = "tts")]
    let tts = TtsFrontend::new(&xns).unwrap();

    // create a policy menu object
    #[cfg(feature = "policy-menu")]
    {
        let mut menu_items = Vec::<MenuItem>::new();
        menu_items.push(MenuItem {
            name: String::from(t!("rootkeys.policy_keep", locales::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([
                PasswordRetentionPolicy::AlwaysKeep.to_u32().unwrap(),
                0,
                0,
                0,
            ]),
            close_on_select: true,
        });
        menu_items.push(MenuItem {
            name: String::from(t!("rootkeys.policy_suspend", locales::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([
                PasswordRetentionPolicy::EraseOnSuspend.to_u32().unwrap(),
                0,
                0,
                0,
            ]),
            close_on_select: true,
        });
        menu_items.push(MenuItem {
            name: String::from(t!("rootkeys.policy_clear", locales::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([
                PasswordRetentionPolicy::AlwaysPurge.to_u32().unwrap(),
                0,
                0,
                0,
            ]),
            close_on_select: true,
        });
        gam::menu_matic(menu_items, crate::ROOTKEY_MENU_NAME, None);
        let mut policy_followup_action: Option<usize> = None;
    }

    // create our very own password modal -- so that critical passwords aren't being shuffled between servers
    // left and right
    let mut password_action = TextEntry::new(
        true,
        TextEntryVisibility::LastChars,
        main_cid,
        Opcode::UxInitUpdateFirstPasswordReturn.to_u32().unwrap(),
        vec![TextEntryPayload::new()],
        None,
    );
    password_action.reset_action_payloads(1, None);
    let mut dismiss_modal_action = Notification::new(main_cid, Opcode::UxGutter.to_u32().unwrap());
    dismiss_modal_action.set_is_password(true);

    let mut rootkeys_modal = Modal::new(
        gam::ROOTKEY_MODAL_NAME,
        ActionType::TextEntry(password_action.clone()),
        Some(t!("rootkeys.updatefirstpass", locales::LANG)),
        None,
        gam::SYSTEM_STYLE,
        8,
    );
    rootkeys_modal.spawn_helper(
        keys_sid,
        rootkeys_modal.sid,
        Opcode::ModalRedraw.to_u32().unwrap(),
        Opcode::ModalKeys.to_u32().unwrap(),
        Opcode::ModalDrop.to_u32().unwrap(),
    );

    // a modals manager for less-secure, run-of-the-mill operations
    let modals = modals::Modals::new(&xns).expect("can't connect to Modals server");
    #[cfg(feature = "policy-menu")]
    let gam = gam::Gam::new(&xns).expect("couldn't establish connection to GAM");

    let mut reboot_initiated = false;
    let mut aes_sender: Option<xous::MessageSender> = None;
    let mut backup_header: Option<BackupHeader> = None;
    let mut deferred_response: Option<xous::MessageSender> = None;
    let mut checksums: Option<Checksums> = None; // storage for PDDB backup checksums
    loop {
        let mut msg = xous::receive_message(keys_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                keys.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                keys.resume();
            }),
            Some(Opcode::KeysInitialized) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.is_initialized() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::IsEfuseSecured) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if let Some(secured) = keys.is_efuse_secured() {
                    if secured {
                        xous::return_scalar(msg.sender, 1).unwrap();
                    } else {
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }
                } else {
                    xous::return_scalar(msg.sender, 2).unwrap();
                }
            }),
            Some(Opcode::IsJtagWorking) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.is_jtag_working() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::ClearPasswordCacheEntry) => {
                msg_blocking_scalar_unpack!(msg, pass_type_code, _, _, _, {
                    let pass_type: AesRootkeyType =
                        FromPrimitive::from_usize(pass_type_code).unwrap_or(AesRootkeyType::NoneSpecified);
                    keys.purge_user_password(pass_type);
                    xous::return_scalar(msg.sender, 1).unwrap();
                })
            }

            // UX flow opcodes
            Some(Opcode::UxTryInitKeys) => {
                match msg.body {
                    xous::Message::BlockingScalar(_) => {
                        deferred_response = Some(msg.sender);
                    }
                    _ => (),
                }
                if false {
                    // short-circuit for testing subroutines
                    let _success = keys.test(&mut rootkeys_modal, main_cid);

                    keys.finish_key_init();
                    log::info!("going to into reboot arc");
                    send_message(
                        main_cid,
                        xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .expect("couldn't initiate dialog box");
                    if let Some(dr) = deferred_response.take() {
                        xous::return_scalar(dr, 0).unwrap();
                    }
                    continue;
                } else {
                    // overall flow:
                    //  - setup the init
                    //  - check that the user is ready to proceed
                    //  - prompt for root password
                    //  - prompt for boot password
                    //  - create the keys
                    //  - write the keys
                    //  - clear the passwords
                    //  - reboot
                    // the following keys should be provisioned:
                    // - self-signing private key
                    // - self-signing public key
                    // - user root key
                    // - pepper

                    if keys.is_initialized() {
                        modals
                            .show_notification(t!("rootkeys.already_init", locales::LANG), None)
                            .expect("modals error");
                        #[cfg(feature = "tts")]
                        tts.tts_blocking(t!("rootkeys.already_init", locales::LANG)).unwrap();
                        keys.set_ux_password_type(None);
                        if let Some(dr) = deferred_response.take() {
                            xous::return_scalar(dr, 0).unwrap();
                        }
                        continue;
                    } else {
                        modals
                            .add_list_item(t!("rootkeys.confirm.yes", locales::LANG))
                            .expect("modals error");
                        modals.add_list_item(t!("rootkeys.confirm.no", locales::LANG)).expect("modals error");
                        if deferred_response.is_some() {
                            modals
                                .add_list_item(t!("rootkeys.confirm.dont_ask", locales::LANG))
                                .expect("modals error");
                            log::info!("{}ROOTKEY.INITQ3,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        } else {
                            log::info!("{}ROOTKEY.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        }
                        match modals.get_radiobutton(t!("rootkeys.confirm", locales::LANG)) {
                            Ok(response) => {
                                if response == t!("rootkeys.confirm.no", locales::LANG) {
                                    if let Some(dr) = deferred_response.take() {
                                        xous::return_scalar(dr, 0).unwrap();
                                    }
                                    continue;
                                } else if response == t!("rootkeys.confirm.dont_ask", locales::LANG) {
                                    keys.set_dont_ask_init();
                                    if let Some(dr) = deferred_response.take() {
                                        xous::return_scalar(dr, 0).unwrap();
                                    }
                                    continue;
                                } else if response != t!("rootkeys.confirm.yes", locales::LANG) {
                                    log::error!("Got unexpected response: {:?}", response);
                                    if let Some(dr) = deferred_response.take() {
                                        xous::return_scalar(dr, 0).unwrap();
                                    }
                                    continue;
                                } else {
                                    // do nothing, this is the forward path
                                }
                            }
                            _ => log::error!("get_radiobutton failed"),
                        }
                    }
                    // setup_key_init() prepares the salt and other items necessary to receive a password
                    // safely
                    keys.setup_key_init();
                    // request the update password first
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    // pop up our private password dialog box
                    password_action
                        .set_action_opcode(Opcode::UxInitUpdateFirstPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.updatefirstpass", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.updatefirstpass", locales::LANG)).unwrap();
                    log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    rootkeys_modal.activate();
                }
            }
            Some(Opcode::UxInitUpdateFirstPasswordReturn) => {
                // assume:
                //   - setup_key_init has also been called (exactly once, before anything happens)
                //   - set_ux_password_type has been called already
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                keys.set_ux_password_type(Some(PasswordType::Update));
                // pop up our private password dialog box
                password_action.set_action_opcode(Opcode::UxInitUpdatePasswordReturn.to_u32().unwrap());
                rootkeys_modal.modify(
                    Some(ActionType::TextEntry(password_action.clone())),
                    Some(t!("rootkeys.updatepass", locales::LANG)),
                    false,
                    None,
                    true,
                    None,
                );
                #[cfg(feature = "tts")]
                tts.tts_blocking(t!("rootkeys.updatepass", locales::LANG)).unwrap();
                log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                rootkeys_modal.activate();
            }
            Some(Opcode::UxInitUpdatePasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                if !keys.hash_and_save_password(plaintext_pw.first().as_str(), true) {
                    // password did not verify
                    modals.add_list_item(t!("rootkeys.gwup.yes", locales::LANG)).expect("modals error");
                    modals.add_list_item(t!("rootkeys.gwup.no", locales::LANG)).expect("modals error");
                    match modals.get_radiobutton(t!("rootkeys.updatepass_fail", locales::LANG)) {
                        Ok(response) => {
                            if response == t!("rootkeys.gwup.yes", locales::LANG) {
                                keys.purge_password(PasswordType::Update);
                                // request the update password first
                                keys.set_ux_password_type(Some(PasswordType::Update));
                                // pop up our private password dialog box
                                password_action.set_action_opcode(
                                    Opcode::UxInitUpdateFirstPasswordReturn.to_u32().unwrap(),
                                );
                                rootkeys_modal.modify(
                                    Some(ActionType::TextEntry(password_action.clone())),
                                    Some(t!("rootkeys.updatefirstpass", locales::LANG)),
                                    false,
                                    None,
                                    true,
                                    None,
                                );
                                #[cfg(feature = "tts")]
                                tts.tts_blocking(t!("rootkeys.updatefirstpass", locales::LANG)).unwrap();
                                log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                rootkeys_modal.activate();
                                continue;
                            } else {
                                // abort
                                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                                buf.volatile_clear();
                                keys.purge_password(PasswordType::Update);
                                keys.purge_sensitive_data();
                                susres.set_suspendable(true).expect("couldn't unblock suspend/resume");
                                if let Some(dr) = deferred_response.take() {
                                    xous::return_scalar(dr, 0).unwrap();
                                }
                                continue;
                            }
                        }
                        _ => {
                            log::error!("get_radiobutton failed");
                            plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                            buf.volatile_clear();
                            keys.purge_password(PasswordType::Update);
                            keys.purge_sensitive_data();
                            susres.set_suspendable(true).expect("couldn't unblock suspend/resume");
                            if let Some(dr) = deferred_response.take() {
                                xous::return_scalar(dr, 0).unwrap();
                            }
                            continue;
                        }
                    }
                }

                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                keys.set_ux_password_type(None);

                // this routine will update the rootkeys_modal with the current Ux state
                let result = keys.do_key_init(&mut rootkeys_modal, main_cid);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                log::info!("set_ux_password result: {:?}", result);

                // clear all the state, re-enable suspend/resume
                keys.finish_key_init();

                match result {
                    Ok(_) => {
                        log::info!("going to into reboot arc");
                        keys.pddb_recycle(); // we have brand new root keys from e.g. a factory reset -- recycle the PDDB, as it is no longer mountable.
                        send_message(
                            main_cid,
                            xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't initiate dialog box");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
                if let Some(dr) = deferred_response.take() {
                    xous::return_scalar(dr, 0).unwrap();
                }
            }
            Some(Opcode::UxTryReboot) => {
                log::info!("{}ROOTKEY.INITDONE,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                log::info!("entering reboot handler");
                // ensure the boost is off so that the reboot will not fail
                com.set_boost(false).unwrap();
                llio.boost_on(false).unwrap();
                ticktimer.sleep_ms(50).unwrap(); // give some time for the voltage to move

                let vbus = (llio.adc_vbus().unwrap() as u32) * 503;
                log::info!("Vbus is: {}mV", vbus / 100);
                if vbus > 150_000 {
                    // 1.5V
                    // if power is plugged in, request that it be removed
                    modals
                        .show_notification(t!("rootkeys.init.unplug_power", locales::LANG), None)
                        .expect("modals error");
                    log::info!("vbus is high, holding off on reboot");
                    send_message(
                        main_cid,
                        xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .expect("couldn't initiate dialog box");
                } else {
                    log::info!("initiating reboot");
                    modals
                        .dynamic_notification(Some(t!("rootkeys.init.finished", locales::LANG)), None)
                        .expect("modals error");
                    xous::yield_slice(); // these are necessary to get the messages in place to do a full redraw before the reboot happens
                    log::info!("going to reboot state");
                    send_message(
                        main_cid,
                        xous::Message::new_scalar(Opcode::UxDoReboot.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .expect("couldn't initiate dialog box");
                }
            }
            Some(Opcode::UxDoReboot) => {
                ticktimer.sleep_ms(1500).unwrap();
                if !reboot_initiated {
                    // set a wakeup alarm a couple seconds from now -- this is the coldboot
                    llio.set_wakeup_alarm(5).unwrap();

                    // allow EC to snoop, so that it can wake up the system
                    llio.allow_ec_snoop(true).unwrap();
                    // allow the EC to power me down
                    llio.allow_power_off(true).unwrap();
                    // now send the power off command
                    susres.immediate_poweroff().unwrap();

                    log::info!("rebooting now!");
                    reboot_initiated = true;
                    ticktimer.sleep_ms(2000).unwrap();
                }

                // refresh the message if it goes away
                send_message(
                    main_cid,
                    xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate dialog box");
            }
            Some(Opcode::UxBlindCopy) => {
                modals.add_list_item(t!("rootkeys.gwup.yes", locales::LANG)).expect("modals error");
                modals.add_list_item(t!("rootkeys.gwup.no", locales::LANG)).expect("modals error");
                match modals.get_radiobutton(t!("rootkeys.blind_update", locales::LANG)) {
                    Ok(response) => {
                        if response == t!("rootkeys.gwup.no", locales::LANG) {
                            continue;
                        }
                        if response != t!("rootkeys.gwup.yes", locales::LANG) {
                            log::error!("got unexpected response from radio box: {:?}", response);
                            continue;
                        } else {
                            // just proceed forward!
                        }
                    }
                    _ => {
                        log::error!("modals error, aborting");
                        continue;
                    }
                }

                let result = keys.do_gateware_provision_uninitialized(&mut rootkeys_modal, main_cid);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals
                            .show_notification(t!("rootkeys.gwup.finished", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
            }
            Some(Opcode::UxUpdateGateware) => {
                match msg.body {
                    xous::Message::BlockingScalar(_) => {
                        deferred_response = Some(msg.sender);
                    }
                    _ => (),
                }
                // steps:
                //  - check update signature "Inspecting gateware update, this will take a moment..."
                //  - if no signature found: "No valid update found! (ok -> exit out)"
                //  - inform user of signature status "Gatware signed with foo, do you want to see the
                //    metadata? (quick/all/no)"
                //  - option to show metadata (multiple pages)
                //  - proceed with update question "Proceed with update? (yes/no)"
                //  - do the update
                modals
                    .dynamic_notification(Some(t!("rootkeys.gwup.inspecting", locales::LANG)), None)
                    .expect("modals error");

                let prompt = match keys.check_gateware_signature(GatewareRegion::Staging) {
                    SignatureResult::SelfSignOk => t!("rootkeys.gwup.viewinfo_ss", locales::LANG),
                    SignatureResult::ThirdPartyOk => t!("rootkeys.gwup.viewinfo_tp", locales::LANG),
                    SignatureResult::DevKeyOk => t!("rootkeys.gwup.viewinfo_dk", locales::LANG),
                    SignatureResult::MalformedSignature
                    | SignatureResult::InvalidPubKey
                    | SignatureResult::InvalidSignatureType => {
                        modals.dynamic_notification_close().expect("modals error");
                        modals
                            .show_notification(t!("rootkeys.gwup.sig_problem", locales::LANG), None)
                            .expect("modals error");
                        if let Some(dr) = deferred_response.take() {
                            xous::return_scalar(dr, 0).unwrap();
                        }
                        continue;
                    }
                    _ => {
                        modals.dynamic_notification_close().expect("modals error");
                        modals
                            .show_notification(t!("rootkeys.gwup.no_update_found", locales::LANG), None)
                            .expect("modals error");
                        if let Some(dr) = deferred_response.take() {
                            xous::return_scalar(dr, 0).unwrap();
                        }
                        continue;
                    }
                };
                modals.dynamic_notification_close().expect("modals error");

                modals.add_list_item(t!("rootkeys.gwup.short", locales::LANG)).expect("modals error");
                modals.add_list_item(t!("rootkeys.gwup.details", locales::LANG)).expect("modals error");
                modals.add_list_item(t!("rootkeys.gwup.none", locales::LANG)).expect("modals error");

                let gw_info = keys.fetch_gw_metadata(GatewareRegion::Staging);
                let info = if gw_info.git_commit == 0 && gw_info.git_additional == 0 {
                    format!(
                        "v{}.{}.{}+{}\nClean tag\n@{}\n{}",
                        gw_info.git_maj,
                        gw_info.git_min,
                        gw_info.git_rev,
                        gw_info.git_additional,
                        str::from_utf8(&gw_info.host_str[..gw_info.host_len as usize]).unwrap(),
                        str::from_utf8(&gw_info.date_str[..gw_info.date_len as usize]).unwrap()
                    )
                } else {
                    format!(
                        "v{}.{}.{}+{}\ncommit: g{:x}\n@{}\n{}",
                        gw_info.git_maj,
                        gw_info.git_min,
                        gw_info.git_rev,
                        gw_info.git_additional,
                        gw_info.git_commit,
                        str::from_utf8(&gw_info.host_str[..gw_info.host_len as usize]).unwrap(),
                        str::from_utf8(&gw_info.date_str[..gw_info.date_len as usize]).unwrap()
                    )
                };

                let mut skip_confirmation = false;
                log::info!("{}ROOTKEY.GWUP,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                match modals.get_radiobutton(prompt) {
                    Ok(response) => {
                        if response == t!("rootkeys.gwup.short", locales::LANG) {
                            modals.show_notification(info.as_str(), None).expect("modals error");
                        } else if response == t!("rootkeys.gwup.details", locales::LANG) {
                            modals.show_notification(info.as_str(), None).expect("modals error");
                            let gw_info = keys.fetch_gw_metadata(GatewareRegion::Staging);
                            // truncate the message to better fit in the rendering box
                            let info_len = if gw_info.log_len > 256 { 256 } else { gw_info.log_len };
                            let info =
                                format!("{}", str::from_utf8(&gw_info.log_str[..info_len as usize]).unwrap());
                            modals.show_notification(info.as_str(), None).expect("modals error");
                            // truncate the message to better fit in the rendering box
                            let status_len = if gw_info.status_len > 256 { 256 } else { gw_info.status_len };
                            let info = format!(
                                "{}",
                                str::from_utf8(&gw_info.status_str[..status_len as usize]).unwrap()
                            );
                            modals.show_notification(info.as_str(), None).expect("modals error");
                        } else {
                            skip_confirmation = true;
                        }
                    }
                    _ => {
                        log::error!("get_radiobutton failed");
                        if let Some(dr) = deferred_response.take() {
                            xous::return_scalar(dr, 0).unwrap();
                        }
                        continue;
                    }
                }
                if !skip_confirmation {
                    modals.add_list_item(t!("rootkeys.gwup.yes", locales::LANG)).expect("modals error");
                    modals.add_list_item(t!("rootkeys.gwup.no", locales::LANG)).expect("modals error");
                    match modals.get_radiobutton(t!("rootkeys.gwup.proceed_confirm", locales::LANG)) {
                        Ok(response) => {
                            if response == t!("rootkeys.gwup.no", locales::LANG) {
                                if let Some(dr) = deferred_response.take() {
                                    xous::return_scalar(dr, 0).unwrap();
                                }
                                continue;
                            }
                            if response != t!("rootkeys.gwup.yes", locales::LANG) {
                                log::error!("got unexpected response from radio box: {:?}", response);
                                if let Some(dr) = deferred_response.take() {
                                    xous::return_scalar(dr, 0).unwrap();
                                }
                                continue;
                            } else {
                                // just proceed forward!
                            }
                        }
                        _ => {
                            log::error!("modals error, aborting");
                            if let Some(dr) = deferred_response.take() {
                                xous::return_scalar(dr, 0).unwrap();
                            }
                            continue;
                        }
                    }
                }

                // here, we always set the password policy to "keep until suspend". Maybe we want to change
                // this, maybe we want to refer to the PDDB to do something different, but in
                // retrospect asking this question to users is dumb and annoying.

                if keys.is_pcache_update_password_valid() {
                    // indicate that there should be no change to the policy
                    let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", locales::LANG));
                    let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                    buf.send(main_cid, Opcode::UxUpdateGwRun.to_u32().unwrap())
                        .map(|_| ())
                        .expect("couldn't send action message");
                } else {
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::UxUpdateGwPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.get_update_password", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.get_update_password", locales::LANG)).unwrap();
                    log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    rootkeys_modal.activate();
                }
            }
            Some(Opcode::UxUpdateGwPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                // indicate that there should be no change to the policy
                let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", locales::LANG));
                let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                buf.send(main_cid, Opcode::UxUpdateGwRun.to_u32().unwrap())
                    .map(|_| ())
                    .expect("couldn't send action message");
            }
            Some(Opcode::UxUpdateGwRun) => {
                // this is a bit of legacy code to handle a return from a menu that would set our password
                // policy. for now, this is short-circuited because every branch that leads
                // into here selects "policy_suspend", which is a 99% correct but 100% more
                // user-friendly policy
                #[cfg(feature = "policy-menu")]
                {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if payload.as_str() == t!("rootkeys.policy_keep", locales::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                    } else if payload.as_str() == t!("rootkeys.policy_suspend", locales::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    } else if payload.as_str() == "no change" {
                        // don't change the policy
                    } else {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                    }
                }
                keys.set_ux_password_type(None);

                let result =
                    keys.do_gateware_update(&mut rootkeys_modal, &modals, main_cid, UpdateType::Regular);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        send_message(
                            main_cid,
                            xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't initiate dialog box");
                        // just do the reboot now.
                        // modals.show_notification(t!("rootkeys.gwup.finished", locales::LANG),
                        // None).expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
                if let Some(dr) = deferred_response.take() {
                    xous::return_scalar(dr, 0).unwrap();
                }
            }
            Some(Opcode::UxSelfSignXous) => {
                if keys.is_pcache_update_password_valid() {
                    // set a default policy
                    let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", locales::LANG));
                    let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                    buf.send(main_cid, Opcode::UxSignXousRun.to_u32().unwrap())
                        .map(|_| ())
                        .expect("couldn't send action message");
                } else {
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::UxSignXousPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.get_signing_password", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.get_signing_password", locales::LANG)).unwrap();
                    log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    rootkeys_modal.activate();
                }
            }
            Some(Opcode::UxSignXousPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", locales::LANG));
                let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                buf.send(main_cid, Opcode::UxSignXousRun.to_u32().unwrap())
                    .map(|_| ())
                    .expect("couldn't send action message");
            }
            Some(Opcode::UxSignXousRun) => {
                #[cfg(feature = "policy-menu")]
                {
                    // legacy code to set policy, if it were to be inserted in the flow
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if payload.as_str() == t!("rootkeys.policy_keep", locales::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                    } else if payload.as_str() == t!("rootkeys.policy_suspend", locales::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    } else if payload.as_str() == "no change" {
                        // don't change the policy
                    } else {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                    }
                }
                keys.set_ux_password_type(None);

                let result = keys.do_sign_xous(&mut rootkeys_modal, main_cid);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals
                            .show_notification(t!("rootkeys.signxous.finished", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
            }
            Some(Opcode::UxAesEnsurePassword) => msg_blocking_scalar_unpack!(msg, key_index, _, _, _, {
                if key_index as u8 == AesRootkeyType::User0.to_u8().unwrap() {
                    if keys.is_pcache_boot_password_valid() {
                        // short circuit the process if the cache is hot
                        xous::return_scalar(msg.sender, 1).unwrap();
                        continue;
                    }
                    if aes_sender.is_some() {
                        log::error!("multiple concurrent requests to UxAesEnsurePasword, not allowed!");
                        xous::return_scalar(msg.sender, 0).unwrap();
                    } else {
                        aes_sender = Some(msg.sender);
                    }
                    keys.set_ux_password_type(Some(PasswordType::Boot));
                    //password_action.set_action_opcode(Opcode::UxAesPasswordPolicy.to_u32().unwrap()); //
                    // skip policy question. it's annoying.
                    password_action.set_action_opcode(Opcode::UxAesEnsureReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.get_login_password", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.get_login_password", locales::LANG)).unwrap();
                    log::info!("{}ROOTKEY.BOOTPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    rootkeys_modal.activate();
                    // note that the scalar is *not* yet returned, it will be returned by the opcode called by
                    // the password assurance
                } else {
                    // insert other indices, as we come to have them in else-ifs
                    // note that there needs to be a way to keep the ensured password in sync with the
                    // actual key index (if multiple passwords are used/required). For now, because there is
                    // only one password, we can use is_pcache_boot_password_valid() to
                    // sync that up; but as we add more keys with more passwords, this
                    // policy may need to become markedly more complicated!

                    // otherwise, an invalid password request
                    modals
                        .show_notification(t!("rootkeys.bad_password_request", locales::LANG), None)
                        .expect("modals error");

                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::UxAesPasswordPolicy) => {
                // this is bypassed, it's not useful. You basically always only want to retain the password
                // until sleep.
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                let mut confirm_radiobox =
                    gam::modal::RadioButtons::new(main_cid, Opcode::UxAesEnsureReturn.to_u32().unwrap());
                confirm_radiobox.is_password = true;
                confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_suspend", locales::LANG)));
                // confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_clear", locales::LANG))); //
                // this policy makes no sense in the use case of the key
                confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_keep", locales::LANG)));
                rootkeys_modal.modify(
                    Some(ActionType::RadioButtons(confirm_radiobox)),
                    Some(t!("rootkeys.policy_request", locales::LANG)),
                    false,
                    None,
                    true,
                    None,
                );
                #[cfg(feature = "tts")]
                tts.tts_blocking(t!("rootkeys.policy_request", locales::LANG)).unwrap();
                rootkeys_modal.activate();
            }
            Some(Opcode::UxAesEnsureReturn) => {
                if let Some(sender) = aes_sender.take() {
                    xous::return_scalar(sender, 1).unwrap();
                    #[cfg(feature = "policy-menu")]
                    {
                        // in case we want to bring back the policy check
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                        if payload.as_str() == t!("rootkeys.policy_keep", locales::LANG) {
                            keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                        } else if payload.as_str() == t!("rootkeys.policy_suspend", locales::LANG) {
                            keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                        } else if payload.as_str() == "no change" {
                            // don't change the policy
                        } else {
                            keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                        }
                    }
                    {
                        let mut buf =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();
                        keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                        plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                        buf.volatile_clear();

                        // this is a reasonable default policy -- don't bother the user to answer this
                        // question all the time.
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    }

                    keys.set_ux_password_type(None);
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                    log::warn!("UxAesEnsureReturn detected a fat-finger event. Ignoring.");
                }
            }
            Some(Opcode::AesOracle) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                // as_flat saves a copy step, but we have to deserialize some enums manually
                let mut aes_op = buffer.to_original::<AesOp, _>().unwrap();
                let op = match aes_op.aes_op {
                    // seems stupid, but we have to do this because we want to have zeroize on the AesOp
                    // record, and it means we can't have Copy on this.
                    AesOpType::Decrypt => AesOpType::Decrypt,
                    AesOpType::Encrypt => AesOpType::Encrypt,
                };
                // deserialize the specifier
                match aes_op.block {
                    AesBlockType::SingleBlock(mut b) => {
                        keys.aes_op(aes_op.key_index, op, &mut b);
                        aes_op.block = AesBlockType::SingleBlock(b);
                    }
                    AesBlockType::ParBlock(mut pb) => {
                        keys.aes_par_op(aes_op.key_index, op, &mut pb);
                        aes_op.block = AesBlockType::ParBlock(pb);
                    }
                };
                buffer.replace(aes_op).unwrap();
            }
            Some(Opcode::AesKwp) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut kwp = buffer.to_original::<KeyWrapper, _>().unwrap();
                keys.kwp_op(&mut kwp);
                buffer.replace(kwp).unwrap();
            }

            Some(Opcode::BbramProvision) => {
                modals
                    .show_notification(t!("rootkeys.bbram.confirm", locales::LANG), None)
                    .expect("modals error");
                let console_input =
                    gam::modal::ConsoleInput::new(main_cid, Opcode::UxBbramCheckReturn.to_u32().unwrap());
                rootkeys_modal.modify(
                    Some(ActionType::ConsoleInput(console_input)),
                    Some(t!("rootkeys.console_input", locales::LANG)),
                    false,
                    None,
                    true,
                    None,
                );
                #[cfg(feature = "tts")]
                tts.tts_blocking(t!("rootkeys.console_input", locales::LANG)).unwrap();
                rootkeys_modal.activate();
                log::info!("{}check_conn", CONSOLE_SENTINEL);
            }
            Some(Opcode::UxBbramCheckReturn) => {
                let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let console_text = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();
                log::info!("got console text:{}", console_text.first().as_str());
                if console_text.first().as_str().starts_with("HELPER_OK") {
                    log::info!("got '{}', moving on", console_text.first().as_str());
                    // proceed
                    if keys.is_pcache_update_password_valid() || !keys.is_initialized() {
                        send_message(
                            main_cid,
                            xous::Message::new_scalar(Opcode::UxBbramRun.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .unwrap();
                    } else {
                        keys.set_ux_password_type(Some(PasswordType::Update));
                        password_action.set_action_opcode(Opcode::UxBbramPasswordReturn.to_u32().unwrap());
                        rootkeys_modal.modify(
                            Some(ActionType::TextEntry(password_action.clone())),
                            Some(t!("rootkeys.get_signing_password", locales::LANG)),
                            false,
                            None,
                            true,
                            None,
                        );
                        #[cfg(feature = "tts")]
                        tts.tts_blocking(t!("rootkeys.get_signing_password", locales::LANG)).unwrap();
                        rootkeys_modal.activate();
                    }
                } else {
                    modals
                        .show_notification(t!("rootkeys.bbram.no_helper", locales::LANG), None)
                        .expect("modals error");
                    continue;
                }
            }
            Some(Opcode::UxBbramPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                send_message(
                    main_cid,
                    xous::Message::new_scalar(Opcode::UxBbramRun.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .unwrap();
            }
            Some(Opcode::UxBbramRun) => {
                keys.set_ux_password_type(None);
                let result = keys.do_gateware_update(
                    &mut rootkeys_modal,
                    &modals,
                    main_cid,
                    UpdateType::BbramProvision,
                );
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals
                            .show_notification(t!("rootkeys.bbram.finished", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
            }

            Some(Opcode::CheckGatewareSignature) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.is_initialized() {
                    if keys.verify_gateware_self_signature(None) {
                        xous::return_scalar(msg.sender, 1).expect("couldn't send return value");
                    } else {
                        xous::return_scalar(msg.sender, 0).expect("couldn't send return value");
                    }
                } else {
                    xous::return_scalar(msg.sender, 2).expect("couldn't send return value");
                }
            }),
            Some(Opcode::StagedSemver) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let staged_semver: [u8; 16] = keys.staged_semver().into();
                xous::return_scalar2(
                    msg.sender,
                    u32::from_le_bytes(staged_semver[0..4].try_into().unwrap()) as usize,
                    u32::from_le_bytes(staged_semver[4..8].try_into().unwrap()) as usize,
                )
                .expect("couldn't send return value");
            }),
            Some(Opcode::TryNoKeySocUpdate) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.try_nokey_soc_update(&mut rootkeys_modal, main_cid) {
                    send_message(
                        main_cid,
                        xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .expect("couldn't initiate dialog box");
                    // the status flow remains blocked "forever", but that's ok -- we should be rebooting.
                    // xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::ShouldPromptForUpdate) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.should_prompt_for_update() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::SetPromptForUpdate) => msg_scalar_unpack!(msg, state, _, _, _, {
                keys.set_prompt_for_update(if state == 1 { true } else { false });
            }),
            Some(Opcode::ShouldRestore) => {
                let mut buf =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let maybe_header = keys.read_backup_header();
                let mut ret = BackupHeaderIpc::default();
                if let Some(header) = maybe_header {
                    let mut data = [0u8; core::mem::size_of::<BackupHeader>()];
                    data.copy_from_slice(header.as_ref());
                    ret.data = Some(data);
                }
                buf.replace(ret).unwrap();
            }
            Some(Opcode::CreateBackup) => {
                let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let ipc = buf.to_original::<BackupHeaderIpc, _>().unwrap();
                if let Some(d) = ipc.data {
                    let mut header = BackupHeader::default();
                    header.as_mut().copy_from_slice(&d);
                    backup_header = Some(header);
                } else {
                    log::error!("Create backup was called, but no header data was provided");
                    continue;
                }
                checksums = ipc.checksums;

                keys.set_ux_password_type(Some(PasswordType::Update));
                password_action.set_action_opcode(Opcode::UxCreateBackupPwReturn.to_u32().unwrap());
                rootkeys_modal.modify(
                    Some(ActionType::TextEntry(password_action.clone())),
                    Some(t!("rootkeys.get_update_password_backup", locales::LANG)),
                    false,
                    None,
                    true,
                    None,
                );
                #[cfg(feature = "tts")]
                tts.tts_blocking(t!("rootkeys.get_update_password_backup", locales::LANG)).unwrap();
                log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                rootkeys_modal.activate();
            }
            Some(Opcode::UxCreateBackupPwReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                keys.set_ux_password_type(None);

                // check if the entered password is valid.
                if let Some((fpga_key, keyrom)) = keys.get_backup_key() {
                    // this gets shown in an "insecure" modal but -- we're expatriating this data anyways, so
                    // meh?
                    modals
                        .show_bip39(Some(t!("rootkeys.backup_key", locales::LANG)), &fpga_key.0.to_vec())
                        .ok();
                    // let the user confirm the key, or skip it. YOLO!
                    loop {
                        modals.add_list_item(t!("rootkeys.gwup.yes", locales::LANG)).expect("modals error");
                        modals.add_list_item(t!("rootkeys.gwup.no", locales::LANG)).expect("modals error");
                        log::info!("{}ROOTKEY.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        match modals.get_radiobutton(t!("rootkeys.backup_verify", locales::LANG)) {
                            Ok(response) => {
                                if response == t!("rootkeys.gwup.yes", locales::LANG) {
                                    match modals
                                        .input_bip39(Some(t!("rootkeys.backup_key_enter", locales::LANG)))
                                    {
                                        Ok(verify) => {
                                            log::debug!("got bip39 verification: {:x?}", verify);
                                            if &verify == &fpga_key.0 {
                                                log::debug!("verify succeeded");
                                                modals
                                                    .show_notification(
                                                        t!("rootkeys.backup_key_match", locales::LANG),
                                                        None,
                                                    )
                                                    .ok();
                                                break;
                                            } else {
                                                log::debug!("verify failed");
                                                modals
                                                    .show_bip39(
                                                        Some(t!(
                                                            "rootkeys.backup_key_mismatch",
                                                            locales::LANG
                                                        )),
                                                        &fpga_key.0.to_vec(),
                                                    )
                                                    .ok();
                                            }
                                        }
                                        _ => {
                                            log::debug!("bip39 verification aborted");
                                            modals
                                                .show_bip39(
                                                    Some(t!("rootkeys.backup_key_mismatch", locales::LANG)),
                                                    &fpga_key.0.to_vec(),
                                                )
                                                .ok();
                                        }
                                    }
                                } else {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                    // now write out the backup
                    let backup_ct = backups::create_backup(fpga_key, backup_header.unwrap(), keyrom);
                    // this final statement has a take/unwrap to set backup_header back to None
                    match keys.write_backup(backup_header.take().unwrap(), backup_ct, checksums.take()) {
                        Ok(_) => {
                            // this switchover takes a couple seconds, give some user feedback
                            modals
                                .dynamic_notification(
                                    Some(t!("rootkeys.backup_prepwait", locales::LANG)),
                                    None,
                                )
                                .ok();
                            let usbd = usb_device_xous::UsbHid::new();
                            usbd.switch_to_core(usb_device_xous::UsbDeviceType::Debug).unwrap();
                            usbd.debug_usb(Some(false)).unwrap();
                            modals.dynamic_notification_close().ok();
                            // there will be a bit of a pause while the QR text renders, but we'll have to fix
                            // that with other optimizations...
                            log::info!("{}BACKUP.STAGED,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                            modals
                                .show_notification(
                                    t!("rootkeys.backup_staged", locales::LANG),
                                    Some("https://github.com/betrusted-io/betrusted-wiki/wiki/Backups"),
                                )
                                .ok();
                            // this informs users who dismiss the QR code that they must reboot to resume
                            // normal operation
                            modals.show_notification(t!("rootkeys.backup_waiting", locales::LANG), None).ok();
                        }
                        Err(_) => {
                            modals
                                .show_notification(t!("rootkeys.backup_staging_error", locales::LANG), None)
                                .ok();
                        }
                    }
                } else {
                    modals.show_notification(t!("rootkeys.backup_badpass", locales::LANG), None).ok();
                    // Prompt the user again for the password. It will do this until a correct password is
                    // entered. maybe it would be thoughtful to add a yes/no box for
                    // rebooting in case someone decides they don't want to run the backup
                    // right now. But I think this is an edge case that few will run into.
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::UxCreateBackupPwReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.get_update_password_backup", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.get_update_password_backup", locales::LANG)).unwrap();
                    log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    rootkeys_modal.activate();
                }
            }
            Some(Opcode::DoRestore) => {
                // check to see if the device was already initialized. If so, warn the user.
                deferred_response = Some(msg.sender);
                if keys.is_initialized() {
                    modals.add_list_item(t!("rootkeys.confirm.yes", locales::LANG)).expect("modals error");
                    modals.add_list_item(t!("rootkeys.confirm.no", locales::LANG)).expect("modals error");
                    log::info!("{}ROOTKEY.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    match modals.get_radiobutton(t!("rootkeys.restore_already_init", locales::LANG)) {
                        Ok(response) => {
                            if response == t!("rootkeys.confirm.no", locales::LANG) {
                                modals
                                    .show_notification(t!("rootkeys.restore_abort", locales::LANG), None)
                                    .ok();
                                if let Some(sender) = deferred_response {
                                    xous::return_scalar(sender, 1).ok();
                                }
                                continue;
                            }
                        }
                        _ => (),
                    }
                }
                // try the '0' key and skip key entry (developers will not have burned an AES key)
                let mut showed_zero_key_notice = false;
                if let Ok((mut header, ct)) = keys.read_backup() {
                    if header.op != BackupOp::Restore {
                        log::warn!("Header op was not Restore. Found {:?} instead. Aborting!", header.op);
                        modals.show_notification(t!("rootkeys.restore_corrupt", locales::LANG), None).ok();
                        if let Some(sender) = deferred_response {
                            xous::return_scalar(sender, 1).ok();
                        }
                        continue;
                    }
                    let mut restore_key = backups::BackupKey::default(); // default is the 0 key
                    // trial restore from 0 key to see if we can skip password entry.
                    if backups::restore_backup(&restore_key, &ct).is_none() {
                        // the '0' key didn't work. get the key from the user
                        match modals.input_bip39(Some(t!("rootkeys.backup_key_enter", locales::LANG))) {
                            Ok(key) => {
                                restore_key.0.copy_from_slice(&key);
                            }
                            _ => {
                                // key entry failed, aborting.
                                modals
                                    .show_notification(t!("rootkeys.restore_badpass", locales::LANG), None)
                                    .ok();
                            }
                        }
                    } else {
                        // notify of zero key
                        modals.show_notification(t!("rootkeys.restore_zero_key", locales::LANG), None).ok();
                        showed_zero_key_notice = true;
                    }
                    // actual restore
                    if let Some(pt) = backups::restore_backup(&restore_key, &ct) {
                        // everything should match except for the op.
                        header.op = pt.header.op;
                        // now check that the two records are identical
                        if pt.header.deref() != header.deref() {
                            log::warn!("Corruption detected:");
                            log::info!("plaintext header: {:?}", pt.header);
                            log::info!("encrypted header: {:?}", header);
                            modals
                                .show_notification(t!("rootkeys.restore_corrupt", locales::LANG), None)
                                .ok();
                            if let Some(sender) = deferred_response {
                                xous::return_scalar(sender, 1).ok();
                            }
                            continue;
                        }
                        let mut restore_rom = backups::KeyRomExport::default();
                        restore_rom.0.copy_from_slice(&pt.keyrom);

                        // We have a bit of a conundrum in terms of restoring the key, because it can mismatch
                        // what's in the target restore hardware. Here is how it gets
                        // resolved:
                        //  - (dev state) If we booted with zero key, we leave it zero key
                        //  - (restore to blank device) If we booted with zero key, and the image is non-zero
                        //    key, We re-encrypt to the zero key that corresponds to the current device, and
                        //    show a warning that the FPGA key / backup key needs to be set as a separate
                        //    step.
                        //  - (restore to keyed device) If we booted with a non-zero key, this means someone
                        //    had primed a SoC image with an encrypted key to start with, or we are restoring
                        //    to a bootable device that has been keyed. This probably means e-fuses or BBRAM
                        //    was burned. We check that the booted image matches the provided backup key, and
                        //    then encrypt to that key.
                        //
                        // Either way, the flow at this point is similar to doing a gateware update, in that
                        // the source image to apply is a zero-key image in the staging area.
                        if keys.is_zero_key() == Some(true) {
                            // zeroize the restore key to match the device state
                            // (we don't need it anymore, now that the backup has been decrypted!)
                            restore_key.0.copy_from_slice(&[0u8; 32]);
                            // also zeroize the copy in the key rom. It *could* be set from the previous FPGA
                            // config, but, since this FPGA we're going to will
                            // have the 0-key, we wipe so the state is consistent.
                            restore_rom.0[0..8].copy_from_slice(&[0u32; 8]);
                        }
                        if restore_key.0 == [0u8; 32] {
                            if !showed_zero_key_notice {
                                modals
                                    .show_notification(
                                        t!("rootkeys.restore_needs_keyburn", locales::LANG),
                                        None,
                                    )
                                    .ok();
                            }
                        }
                        // this will setup the pepper so we can request a password
                        keys.setup_restore_init(restore_key, restore_rom);

                        // get the update password, so we can encrypt the FPGA key to it.
                        keys.set_ux_password_type(Some(PasswordType::Update));
                        password_action.set_action_opcode(Opcode::UxDoRestorePwReturn.to_u32().unwrap());
                        rootkeys_modal.modify(
                            Some(ActionType::TextEntry(password_action.clone())),
                            Some(t!("rootkeys.get_update_password_restore", locales::LANG)),
                            false,
                            None,
                            true,
                            None,
                        );
                        #[cfg(feature = "tts")]
                        tts.tts_blocking(t!("rootkeys.get_update_password_restore", locales::LANG)).unwrap();
                        log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        rootkeys_modal.activate();
                    } else {
                        // decryption error
                        modals.show_notification(t!("rootkeys.restore_corrupt", locales::LANG), None).ok();
                    }
                } else {
                    log::error!("internal error: couldn't read out the restore region");
                }
            }
            Some(Opcode::UxDoRestorePwReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                keys.set_ux_password_type(None);

                let result =
                    keys.do_gateware_update(&mut rootkeys_modal, &modals, main_cid, UpdateType::Restore);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                log::info!("set_ux_password result: {:?}", result);
                match result {
                    Ok(_) => {
                        // clear all the state, re-enable suspend/resume
                        keys.finish_key_init();

                        let (mut header, ct) = keys.read_backup()
                        .expect("Internal error: we're doing a backup, but somehow the backup record has disappeared on us!");
                        if u64::from_le_bytes(header.dna) == llio.soc_dna().unwrap() {
                            log::info!("erasing backup block");
                            // we're restoring to the same device, we're done!
                            keys.erase_backup();
                        } else {
                            log::info!("DNA is not the same; setting flag to trigger RestoreDna flow");
                            // one more step...have to re-encrypt the PDDB's basis to the new device's DNA.
                            // we'll handle this *after* the reboot, because we may need a newer SoC version,
                            // etc.
                            header.op = BackupOp::RestoreDna; // it gets set inside the call too; could lose this line but testing is painful on this flow.
                            keys.write_restore_dna(header, ct).expect(
                                "Couldn't set flag for restore with new DNA: some data loss will occur.",
                            );
                        }
                        log::info!("going to into reboot arc");
                        send_message(
                            main_cid,
                            xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't initiate dialog box");
                        // don't unblock the caller if we go to the reboot arc
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        // clear all the state, re-enable suspend/resume
                        keys.finish_key_init();

                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                        if let Some(sender) = deferred_response {
                            xous::return_scalar(sender, 1).ok();
                        }
                    }
                    Err(RootkeyResult::KeyError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                        modals.add_list_item(t!("rootkeys.gwup.yes", locales::LANG)).expect("modals error");
                        modals.add_list_item(t!("rootkeys.gwup.no", locales::LANG)).expect("modals error");
                        match modals.get_radiobutton(t!("rootkeys.try_again", locales::LANG)) {
                            Ok(response) => {
                                if response == t!("rootkeys.gwup.no", locales::LANG) {
                                    // clear all the state, re-enable suspend/resume
                                    keys.finish_key_init();

                                    if let Some(dr) = deferred_response.take() {
                                        xous::return_scalar(dr, 1).unwrap();
                                    }
                                    continue;
                                } else if response == t!("rootkeys.gwup.yes", locales::LANG) {
                                    // get the update password, so we can encrypt the FPGA key to it.
                                    keys.set_ux_password_type(Some(PasswordType::Update));
                                    password_action
                                        .set_action_opcode(Opcode::UxDoRestorePwReturn.to_u32().unwrap());
                                    rootkeys_modal.modify(
                                        Some(ActionType::TextEntry(password_action.clone())),
                                        Some(t!("rootkeys.get_update_password_restore", locales::LANG)),
                                        false,
                                        None,
                                        true,
                                        None,
                                    );
                                    #[cfg(feature = "tts")]
                                    tts.tts_blocking(t!(
                                        "rootkeys.get_update_password_restore",
                                        locales::LANG
                                    ))
                                    .unwrap();
                                    log::info!("{}ROOTKEY.UPDPW,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                    rootkeys_modal.activate();
                                } else {
                                    // clear all the state, re-enable suspend/resume
                                    keys.finish_key_init();

                                    log::error!("get_radiobutton had an unexpected response: {:?}", response);
                                }
                            }
                            _ => log::error!("get_radiobutton failed"),
                        }
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        // clear all the state, re-enable suspend/resume
                        keys.finish_key_init();

                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                        if let Some(sender) = deferred_response {
                            xous::return_scalar(sender, 1).ok();
                        }
                    }
                    Err(RootkeyResult::FlashError) => {
                        // clear all the state, re-enable suspend/resume
                        keys.finish_key_init();

                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                        if let Some(sender) = deferred_response {
                            xous::return_scalar(sender, 1).ok();
                        }
                    }
                    Err(RootkeyResult::StateError) => {
                        // clear all the state, re-enable suspend/resume
                        keys.finish_key_init();

                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                        if let Some(sender) = deferred_response {
                            xous::return_scalar(sender, 1).ok();
                        }
                    }
                }
            }
            Some(Opcode::EraseBackupBlock) => {
                keys.erase_backup();
                xous::return_scalar(msg.sender, 1).ok();
            }
            Some(Opcode::IsZeroKey) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let query = keys.is_zero_key();
                if let Some(q) = query {
                    if q {
                        xous::return_scalar2(msg.sender, 1, 1).ok();
                    } else {
                        xous::return_scalar2(msg.sender, 0, 1).ok();
                    }
                } else {
                    xous::return_scalar2(msg.sender, 0, 0).ok();
                }
            }),
            Some(Opcode::IsDontAskSet) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.is_dont_ask_init_set() {
                    xous::return_scalar(msg.sender, 1).ok();
                } else {
                    xous::return_scalar(msg.sender, 0).ok();
                }
            }),
            Some(Opcode::ResetDontAsk) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                keys.reset_dont_ask_init();
                xous::return_scalar(msg.sender, 1).ok();
            }),
            #[cfg(feature = "efuse")]
            Some(Opcode::BurnEfuse) => {
                // Flow:
                //   0. confirm that we can read out our JTAG ID (make sure the interface is working)
                //   1. get password
                //   [inside do_gateware_update()]
                //   2. create a backup of the gateware
                //   3. inject new key into gateware, while re-encrypting to the new key
                //   4. burn the key using jtag.efuse_key_burn()
                //   5. seal the device using seal_device_forever() [first device don't call this so we can
                //      debug the flow; on second device, call this and confirm we can't re-burn or readout]
                //   6. force a reboot
                log::info!("{}EFUSE.START,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                // confirm that we can read out our JTAG ID.
                if !keys.is_jtag_working() {
                    modals
                        .show_notification(t!("rootkeys.efuse_jtag_fail", locales::LANG), None)
                        .expect("modals error");
                    continue;
                }
                // get password, or skip getting it if it's already in cache
                if keys.is_pcache_update_password_valid() || !keys.is_initialized() {
                    send_message(
                        main_cid,
                        xous::Message::new_scalar(Opcode::EfuseRun.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                } else {
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::EfusePasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action.clone())),
                        Some(t!("rootkeys.get_signing_password", locales::LANG)),
                        false,
                        None,
                        true,
                        None,
                    );
                    #[cfg(feature = "tts")]
                    tts.tts_blocking(t!("rootkeys.get_signing_password", locales::LANG)).unwrap();
                    rootkeys_modal.activate();
                }
            }
            #[cfg(feature = "efuse")]
            Some(Opcode::EfusePasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let plaintext_pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.first().as_str(), false);
                plaintext_pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                send_message(
                    main_cid,
                    xous::Message::new_scalar(Opcode::EfuseRun.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .unwrap();
            }
            #[cfg(feature = "efuse")]
            Some(Opcode::EfuseRun) => {
                keys.set_ux_password_type(None);
                let result = keys.do_gateware_update(
                    &mut rootkeys_modal,
                    &modals,
                    main_cid,
                    UpdateType::EfuseProvision,
                );
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close
                // and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        log::info!("{}EFUSE.OK,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        modals
                            .show_notification(t!("rootkeys.efuse_finished", locales::LANG), None)
                            .expect("modals error");
                        send_message(
                            main_cid,
                            xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't initiate dialog box");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_alignment", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals
                            .show_notification(t!("rootkeys.init.fail_key", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_verify", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals
                            .show_notification(t!("rootkeys.init.fail_burn", locales::LANG), None)
                            .expect("modals error");
                    }
                    Err(RootkeyResult::StateError) => {
                        modals
                            .show_notification(t!("rootkeys.wrong_state", locales::LANG), None)
                            .expect("modals error");
                    }
                }
            }
            Some(Opcode::TestUx) => msg_blocking_scalar_unpack!(msg, _arg, _, _, _, {
                // dummy test for now
                xous::return_scalar(msg.sender, 1234).unwrap();
            }),
            #[cfg(feature = "policy-menu")]
            Some(Opcode::UxGetPolicy) => {
                gam.raise_menu(ROOTKEY_MENU_NAME).expect("couldn't raise policy menu");
            }
            #[cfg(feature = "policy-menu")]
            Some(Opcode::UxPolicyReturn) => msg_scalar_unpack!(msg, policy_code, _, _, _, {
                keys.update_policy(FromPrimitive::from_usize(policy_code));
                if let Some(action) = policy_followup_action {
                    send_message(main_cid, xous::Message::new_scalar(action, 0, 0, 0, 0)).unwrap();
                }
                policy_followup_action = None;
            }),
            Some(Opcode::UxGutter) => {
                // an intentional NOP for UX actions that require a destintation but need no action
            }

            // boilerplate Ux handlers
            Some(Opcode::ModalRedraw) => {
                rootkeys_modal.redraw();
            }
            Some(Opcode::ModalKeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                rootkeys_modal.key_event(keys);
            }),
            Some(Opcode::ModalDrop) => {
                panic!("Password modal for rootkeys quit unexpectedly")
            }
            Some(Opcode::Quit) => {
                log::warn!("password thread received quit, exiting.");
                break;
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(keys_sid).unwrap();
    xous::destroy_server(keys_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
