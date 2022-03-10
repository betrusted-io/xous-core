#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use xous::{msg_scalar_unpack, send_message, msg_blocking_scalar_unpack};
#[cfg(feature = "policy-menu")]
use xous_ipc::String;

use xous_ipc::Buffer;

use num_traits::*;

use gam::modal::*;
#[cfg(feature = "policy-menu")]
use gam::{MenuItem, MenuPayload};

#[cfg(feature = "tts")]
use tts_frontend::*;

use locales::t;
use std::format;
use std::str;

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation;
#[cfg(any(target_os = "none", target_os = "xous"))]
use implementation::*;
/// used by the bbram helper/console protocol to indicate the start of a console message
const CONSOLE_SENTINEL: &'static str = "CONS_SENTINEL|";

#[cfg(any(target_os = "none", target_os = "xous"))]
mod bcrypt;

pub enum SignatureResult {
    SelfSignOk,
    ThirdPartyOk,
    DevKeyOk,
    Invalid,
}
#[allow(dead_code)]
pub enum GatewareRegion {
    Boot,
    Staging,
}


/// An "easily" parseable metadata structure in flash. There's nothing that guarantees the authenticity
/// of the metadata in and of itself, other than the digital signature that wraps the entire gateware record.
/// Thus we're relying on the person who signs the gateware to not inject false data here.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct MetadataInFlash {
    pub magic: u32,  // 0x6174656d 'atem'
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
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    mod keywrap;
    use keywrap::*;
    use crate::PasswordRetentionPolicy;
    use crate::PasswordType;
    use gam::modal::{Modal, Slider};
    use locales::t;
    use crate::api::*;
    use gam::{ActionType, ProgressBar};
    use num_traits::*;
    use crate::{SignatureResult, GatewareRegion, MetadataInFlash};
    use aes::Aes256;
    use aes::cipher::{BlockDecrypt, BlockEncrypt, NewBlockCipher, generic_array::GenericArray};
    use std::convert::TryInto;

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
    }

    #[allow(dead_code)]
    impl RootKeys {
        pub fn new() -> RootKeys {
            let xns = xous_names::XousNames::new().unwrap();
            let jtag = jtag::Jtag::new(&xns).expect("couldn't connect to jtag server");
            RootKeys {
                password_type: None,
                xns,
                // must occupy tihs connection for the system to boot properly
                jtag,
                ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }

        pub fn update_policy(&mut self, policy: Option<PasswordRetentionPolicy>) {
            log::info!("policy updated: {:?}", policy);
        }
        pub fn hash_and_save_password(&mut self, pw: &str) {
            log::info!("got password plaintext: {}", pw);
        }
        pub fn set_ux_password_type(&mut self, cur_type: Option<PasswordType>) {
            self.password_type = cur_type;
        }
        pub fn is_initialized(&self) -> bool {true}
        pub fn setup_key_init(&mut self) {}
        fn fake_progress(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID, msg: &str) -> Result<(), RootkeyResult> {
            let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
            0, 100, 10, Some("%"), 0, true, true
            );
            progress_action.set_is_password(true);
            // now show the init wait note...
            rootkeys_modal.modify(
                Some(ActionType::Slider(progress_action)),
                Some(msg), false,
                None, true, None);
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
        pub fn do_key_init(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.setup_wait", xous::LANG))
        }
        pub fn do_gateware_update(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID, _provision_bbram: bool) -> Result<(), RootkeyResult> {
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.gwup_starting", xous::LANG))
        }
        pub fn do_sign_xous(&mut self, rootkeys_modal: &mut Modal, main_cid: xous::CID) -> Result<(), RootkeyResult> {
            self.fake_progress(rootkeys_modal, main_cid, t!("rootkeys.init.signing_kernel", xous::LANG))
        }
        pub fn purge_password(&mut self, _ptype: PasswordType) {}
        pub fn purge_user_password(&mut self, _ptype: AesRootkeyType) {}

        pub fn get_ux_password_type(&self) -> Option<PasswordType> {self.password_type}
        pub fn finish_key_init(&mut self) {}
        pub fn verify_gateware_self_signature(&mut self) -> bool {
            true
        }
        pub fn test(&mut self, _rootkeys_modal: &mut Modal, _main_cid: xous::CID) -> Result<(), RootkeyResult> {
            Ok(())
        }
        pub fn is_jtag_working(&self) -> bool {true}
        pub fn is_efuse_secured(&self) -> Option<bool> {None}
        pub fn check_gateware_signature(&mut self, _region_enum: GatewareRegion) -> SignatureResult {
            log::info!("faking gateware check...");
            self.ticktimer.sleep_ms(4000).unwrap();
            log::info!("done");
            SignatureResult::DevKeyOk
        }
        pub fn is_pcache_update_password_valid(&self) -> bool {
            false
        }
        pub fn is_pcache_boot_password_valid(&self) -> bool {
            true
        }
        pub fn fpga_key_source(&self) -> FpgaKeySource {
            FpgaKeySource::Efuse
        }

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
                date_str: [50, 48, 50, 49, 45, 48, 56, 45, 49, 50, 32, 50, 50, 58, 49, 53, 58, 53, 51, 46, 56, 49, 55, 51, 53, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                host_len: 14,
                host_str: [98, 117, 110, 110, 105, 101, 45, 100, 101, 115, 107, 116, 111, 112, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                tag_len: 19,
                tag_str: [118, 48, 46, 56, 46, 50, 45, 55, 49, 45, 103, 102, 102, 98, 97, 52, 55, 102, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                log_len: 203,
                log_str: [99, 111, 109, 109, 105, 116, 32, 102, 102, 98, 97, 52, 55, 102, 52, 98, 102, 55, 99, 52, 51, 50, 55, 54, 55, 50, 50, 56, 102, 101, 99, 52, 51, 53, 97, 56, 56, 48, 54, 54, 55, 53, 101, 52, 102, 49, 102, 10, 65, 117, 116, 104, 111, 114, 58, 32, 98, 117, 110, 110, 105, 101, 32, 60, 98, 117, 110, 110, 105, 101, 64, 107, 111, 115, 97, 103, 105, 46, 99, 111, 109, 62, 10, 68, 97, 116, 101, 58, 32, 32, 32, 84, 104, 117, 32, 65, 117, 103, 32, 49, 50, 32, 48, 52, 58, 52, 49, 58, 53, 49, 32, 50, 48, 50, 49, 32, 43, 48, 56, 48, 48, 10, 10, 32, 32, 32, 32, 109, 111, 100, 105, 102, 121, 32, 98, 111, 111, 116, 32, 116, 111, 32, 100, 111, 32, 102, 97, 108, 108, 98, 97, 99, 107, 32, 111, 110, 32, 115, 105, 103, 110, 97, 116, 117, 114, 101, 115, 10, 10, 77, 9, 98, 111, 111, 116, 47, 98, 101, 116, 114, 117, 115, 116, 101, 100, 45, 98, 111, 111, 116, 47, 115, 114, 99, 47, 109, 97, 105, 110, 46, 114, 115, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                status_len: 512,
                status_str: [79, 110, 32, 98, 114, 97, 110, 99, 104, 32, 109, 97, 105, 110, 10, 89, 111, 117, 114, 32, 98, 114, 97, 110, 99, 104, 32, 105, 115, 32, 117, 112, 32, 116, 111, 32, 100, 97, 116, 101, 32, 119, 105, 116, 104, 32, 39, 111, 114, 105, 103, 105, 110, 47, 109, 97, 105, 110, 39, 46, 10, 10, 67, 104, 97, 110, 103, 101, 115, 32, 110, 111, 116, 32, 115, 116, 97, 103, 101, 100, 32, 102, 111, 114, 32, 99, 111, 109, 109, 105, 116, 58, 10, 32, 32, 40, 117, 115, 101, 32, 34, 103, 105, 116, 32, 97, 100, 100, 32, 60, 102, 105, 108, 101, 62, 46, 46, 46, 34, 32, 116, 111, 32, 117, 112, 100, 97, 116, 101, 32, 119, 104, 97, 116, 32, 119, 105, 108, 108, 32, 98, 101, 32, 99, 111, 109, 109, 105, 116, 116, 101, 100, 41, 10, 32, 32, 40, 117, 115, 101, 32, 34, 103, 105, 116, 32, 114, 101, 115, 116, 111, 114, 101, 32, 60, 102, 105, 108, 101, 62, 46, 46, 46, 34, 32, 116, 111, 32, 100, 105, 115, 99, 97, 114, 100, 32, 99, 104, 97, 110, 103, 101, 115, 32, 105, 110, 32, 119, 111, 114, 107, 105, 110, 103, 32, 100, 105, 114, 101, 99, 116, 111, 114, 121, 41, 10, 32, 32, 40, 99, 111, 109, 109, 105, 116, 32, 111, 114, 32, 100, 105, 115, 99, 97, 114, 100, 32, 116, 104, 101, 32, 117, 110, 116, 114, 97, 99, 107, 101, 100, 32, 111, 114, 32, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 32, 105, 110, 32, 115, 117, 98, 109, 111, 100, 117, 108, 101, 115, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 97, 112, 112, 101, 110, 100, 95, 99, 115, 114, 46, 112, 121, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 101, 116, 114, 117, 115, 116, 101, 100, 95, 115, 111, 99, 46, 112, 121, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 111, 111, 116, 47, 98, 101, 116, 114, 117, 115, 116, 101, 100, 45, 98, 111, 111, 116, 47, 97, 115, 115, 101, 109, 98, 108, 101, 46, 115, 104, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 111, 111, 116, 95, 116, 101, 115, 116, 46, 112, 121, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 98, 117, 105, 108, 100, 45, 100, 111, 99, 115, 46, 115, 104, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 99, 104, 101, 99, 107, 45, 116, 105, 109, 105, 110, 103, 46, 115, 104, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 99, 111, 109, 112, 105, 108, 101, 114, 95, 114, 116, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 101, 110, 99, 114, 121, 112, 116, 45, 98, 105, 116, 115, 116, 114, 101, 97, 109, 45, 112, 121, 116, 104, 111, 110, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 103, 97, 116, 101, 119, 97, 114, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 108, 105, 116, 101, 100, 114, 97, 109, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 108, 105, 116, 101, 115, 99, 111, 112, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 108, 105, 116, 101, 120, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 109, 105, 103, 101, 110, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 111, 112, 101, 110, 116, 105, 116, 97, 110, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 112, 121, 115, 101, 114, 105, 97, 108, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 112, 121, 116, 104, 111, 110, 100, 97, 116, 97, 45, 99, 112, 117, 45, 118, 101, 120, 114, 105, 115, 99, 118, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102, 105, 101, 100, 58, 32, 32, 32, 100, 101, 112, 115, 47, 114, 111, 109, 45, 108, 111, 99, 97, 116, 101, 32, 40, 109, 111, 100, 105, 102, 105, 101, 100, 32, 99, 111, 110, 116, 101, 110, 116, 41, 10, 9, 109, 111, 100, 105, 102],
           }
        }
        pub fn aes_op(&mut self, key_index: u8, op_type: AesOpType, block: &mut [u8; 16]) {
            // fake a "well known" key by just expanding the index into a trivial key
            let mut key = [0 as u8; 32];
            key[0] = key_index;
            let cipher = Aes256::new(GenericArray::from_slice(&key));
            match op_type {
                AesOpType::Decrypt => {
                    cipher.decrypt_block(block.try_into().unwrap())
                },
                AesOpType::Encrypt => {
                    cipher.encrypt_block(block.try_into().unwrap())
                }
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
                },
                AesOpType::Encrypt => {
                    for block in blocks.iter_mut() {
                        cipher.encrypt_block(block.try_into().unwrap());
                    }
                }
            }
        }
        pub fn kwp_op(&mut self, kwp: &mut KeyWrapper) {
            let keywrapper = Aes256KeyWrap::new(&[0u8; 32]);
            match kwp.op {
                KeyWrapOp::Wrap => {
                    match keywrapper.encapsulate(&kwp.data[..kwp.len as usize]) {
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
                            kwp.result = Some(e);
                        }
                    }
                }
                KeyWrapOp::Unwrap => {
                    match keywrapper.decapsulate(&kwp.data[..kwp.len as usize], kwp.expected_len as usize) {
                        Ok(unwrapped) => {
                            for (&src, dst) in unwrapped.iter().zip(kwp.data.iter_mut()) {
                                *dst = src;
                            }
                            kwp.len = unwrapped.len() as u32;
                            kwp.result = None;
                        }
                        Err(e) => {
                            kwp.result = Some(e);
                        }
                    }
                }
            }
        }
    }
}


#[xous::xous_main]
fn xmain() -> ! {
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
    let rtc = llio::Rtc::new(&xns);
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let com = com::Com::new(&xns).unwrap();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let main_cid = xous::connect(keys_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(None, &xns, api::Opcode::SuspendResume as u32, main_cid).expect("couldn't create suspend/resume object");

    #[cfg(feature="tts")]
    let tts = TtsFrontend::new(&xns).unwrap();

    // create a policy menu object
    #[cfg(feature = "policy-menu")]
    {
        let mut menu_items = Vec::<MenuItem>::new();
        menu_items.push(MenuItem {
            name: String::from_str(t!("rootkeys.policy_keep", xous::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysKeep.to_u32().unwrap(), 0, 0, 0,]),
            close_on_select: true,
        });
        menu_items.push(MenuItem {
            name: String::from_str(t!("rootkeys.policy_suspend", xous::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::EraseOnSuspend.to_u32().unwrap(), 0, 0, 0,]),
            close_on_select: true,
        });
        menu_items.push(MenuItem {
            name: String::from_str(t!("rootkeys.policy_clear", xous::LANG)),
            action_conn: Some(main_cid),
            action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
            action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysPurge.to_u32().unwrap(), 0, 0, 0,]),
            close_on_select: true,
        });
        gam::menu_matic(menu_items, crate::ROOTKEY_MENU_NAME, None);
        let mut policy_followup_action: Option<usize> = None;
    }

    // create our very own password modal -- so that critical passwords aren't being shuffled between servers left and right
    let mut password_action = TextEntry {
        is_password: true,
        visibility: TextEntryVisibility::LastChars,
        action_conn: main_cid,
        action_opcode: Opcode::UxInitBootPasswordReturn.to_u32().unwrap(),
        action_payload: TextEntryPayload::new(),
        validator: None,
    };
    let mut dismiss_modal_action = Notification::new(main_cid, Opcode::UxGutter.to_u32().unwrap());
    dismiss_modal_action.set_is_password(true);

    let mut rootkeys_modal = Modal::new(
        gam::ROOTKEY_MODAL_NAME,
        ActionType::TextEntry(password_action),
        Some(t!("rootkeys.bootpass", xous::LANG)),
        None,
        GlyphStyle::Regular,
        8
    );
    rootkeys_modal.spawn_helper(keys_sid, rootkeys_modal.sid,
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
    loop {
        let mut msg = xous::receive_message(keys_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
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
            Some(Opcode::ClearPasswordCacheEntry) => msg_blocking_scalar_unpack!(msg, pass_type_code, _, _, _, {
                let pass_type: AesRootkeyType = FromPrimitive::from_usize(pass_type_code).unwrap_or(AesRootkeyType::NoneSpecified);
                keys.purge_user_password(pass_type);
                xous::return_scalar(msg.sender, 1).unwrap();
            }),

            // UX flow opcodes
            Some(Opcode::UxTryInitKeys) => msg_scalar_unpack!(msg, _, _, _, _, {
                if false { // short-circuit for testing subroutines
                    let _success = keys.test(&mut rootkeys_modal, main_cid);

                    keys.finish_key_init();
                    log::info!("going to into reboot arc");
                    send_message(main_cid,
                        xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0)
                    ).expect("couldn't initiate dialog box");

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
                        modals.show_notification(t!("rootkeys.already_init", xous::LANG)).expect("modals error");
                        #[cfg(feature="tts")]
                        tts.tts_blocking(t!("rootkeys.already_init", xous::LANG)).unwrap();
                        keys.set_ux_password_type(None);
                        continue;
                    } else {
                        modals.add_list_item(t!("rootkeys.confirm.yes", xous::LANG)).expect("modals error");
                        modals.add_list_item(t!("rootkeys.confirm.no", xous::LANG)).expect("modals error");
                        match modals.get_radiobutton(t!("rootkeys.confirm", xous::LANG)) {
                            Ok(response) => {
                                if response == t!("rootkeys.confirm.no", xous::LANG) {
                                    continue;
                                } else if response != t!("rootkeys.confirm.yes", xous::LANG) {
                                    log::error!("Got unexpected response: {:?}", response);
                                    continue;
                                } else {
                                    // do nothing, this is the forward path
                                }
                            }
                            _ => log::error!("get_radiobutton failed"),
                        }
                    }
                    // setup_key_init() prepares the salt and other items necessary to receive a password safely
                    keys.setup_key_init();
                    // request the boot password first
                    keys.set_ux_password_type(Some(PasswordType::Boot));
                    // pop up our private password dialog box
                    password_action.set_action_opcode(Opcode::UxInitBootPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action)),
                        Some(t!("rootkeys.bootpass", xous::LANG)), false,
                        None, true, None
                    );
                    #[cfg(feature="tts")]
                    tts.tts_blocking(t!("rootkeys.bootpass", xous::LANG)).unwrap();
                    rootkeys_modal.activate();
                }
            }),
            Some(Opcode::UxInitBootPasswordReturn) => {
                // assume:
                //   - setup_key_init has also been called (exactly once, before anything happens)
                //   - set_ux_password_type has been called already
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                keys.set_ux_password_type(Some(PasswordType::Update));
                // pop up our private password dialog box
                password_action.set_action_opcode(Opcode::UxInitUpdatePasswordReturn.to_u32().unwrap());
                rootkeys_modal.modify(
                    Some(ActionType::TextEntry(password_action)),
                    Some(t!("rootkeys.updatepass", xous::LANG)), false,
                    None, true, None
                );
                #[cfg(feature="tts")]
                tts.tts_blocking(t!("rootkeys.updatepass", xous::LANG)).unwrap();
                rootkeys_modal.activate();
            },
            Some(Opcode::UxInitUpdatePasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                keys.set_ux_password_type(None);

                // this routine will update the rootkeys_modal with the current Ux state
                let result = keys.do_key_init(&mut rootkeys_modal, main_cid);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                log::info!("set_ux_password result: {:?}", result);

                // clear all the state, re-enable suspend/resume
                keys.finish_key_init();

                match result {
                    Ok(_) => {
                        log::info!("going to into reboot arc");
                        send_message(main_cid,
                            xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0)
                        ).expect("couldn't initiate dialog box");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals.show_notification(t!("rootkeys.init.fail_alignment", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        modals.show_notification(t!("rootkeys.init.fail_key", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals.show_notification(t!("rootkeys.init.fail_verify", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals.show_notification(t!("rootkeys.init.fail_burn", xous::LANG)).expect("modals error");
                    }
                }
            },
            Some(Opcode::UxTryReboot) => {
                log::info!("entering reboot handler");
                // ensure the boost is off so that the reboot will not fail
                com.set_boost(false).unwrap();
                llio.boost_on(false).unwrap();
                ticktimer.sleep_ms(50).unwrap(); // give some time for the voltage to move

                let vbus = (llio.adc_vbus().unwrap() as f64) * 0.005033;
                log::info!("Vbus is: {:.3}V", vbus);
                if vbus > 1.5 {
                    // if power is plugged in, request that it be removed
                    modals.show_notification(t!("rootkeys.init.unplug_power", xous::LANG)).expect("modals error");
                    log::info!("vbus is high, holding off on reboot");
                    send_message(main_cid,
                        xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0)
                    ).expect("couldn't initiate dialog box");
                } else {
                    log::info!("initiating reboot");
                    modals.dynamic_notification(Some(t!("rootkeys.init.finished", xous::LANG)), None).expect("modals error");
                    xous::yield_slice(); // these are necessary to get the messages in place to do a full redraw before the reboot happens
                    log::info!("going to reboot state");
                    send_message(main_cid,
                        xous::Message::new_scalar(Opcode::UxDoReboot.to_usize().unwrap(), 0, 0, 0, 0)
                    ).expect("couldn't initiate dialog box");
                }
            }
            Some(Opcode::UxDoReboot) => {
                ticktimer.sleep_ms(1500).unwrap();
                if !reboot_initiated {
                    // set a wakeup alarm a couple seconds from now -- this is the coldboot
                    rtc.set_wakeup_alarm(3).unwrap();

                    // allow EC to snoop, so that it can wake up the system
                    llio.allow_ec_snoop(true).unwrap();
                    // allow the EC to power me down
                    llio.allow_power_off(true).unwrap();
                    // now send the power off command
                    com.power_off_soc().unwrap(); // not that at this point, the screen freezes with the last thing displayed...

                    log::info!("rebooting now!");
                    reboot_initiated = true;
                }

                // refresh the message if it goes away
                send_message(main_cid,
                    xous::Message::new_scalar(Opcode::UxTryReboot.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate dialog box");
            }
            Some(Opcode::UxUpdateGateware) => {
                // steps:
                //  - check update signature "Inspecting gateware update, this will take a moment..."
                //  - if no signature found: "No valid update found! (ok -> exit out)"
                //  - inform user of signature status "Gatware signed with foo, do you want to see the metadata? (quick/all/no)"
                //  - option to show metadata (multiple pages)
                //  - proceed with update question "Proceed with update? (yes/no)"
                //  - do the update
                modals.dynamic_notification(Some(t!("rootkeys.gwup.inspecting", xous::LANG)), None).expect("modals error");

                let prompt = match keys.check_gateware_signature(GatewareRegion::Staging) {
                    SignatureResult::SelfSignOk => t!("rootkeys.gwup.viewinfo_ss", xous::LANG),
                    SignatureResult::ThirdPartyOk => t!("rootkeys.gwup.viewinfo_tp", xous::LANG),
                    SignatureResult::DevKeyOk => t!("rootkeys.gwup.viewinfo_dk", xous::LANG),
                    _ => {
                        modals.dynamic_notification_close().expect("modals error");
                        modals.show_notification(t!("rootkeys.gwup.no_update_found", xous::LANG)).expect("modals error");
                        continue;
                    }
                };
                modals.dynamic_notification_close().expect("modals error");

                modals.add_list_item(t!("rootkeys.gwup.short", xous::LANG)).expect("modals error");
                modals.add_list_item(t!("rootkeys.gwup.details", xous::LANG)).expect("modals error");
                modals.add_list_item(t!("rootkeys.gwup.none", xous::LANG)).expect("modals error");

                let gw_info = keys.fetch_gw_metadata(GatewareRegion::Staging);
                let info = if gw_info.git_commit == 0 && gw_info.git_additional == 0 {
                    format!("v{}.{}.{}+{}\nClean tag\n@{}\n{}",
                        gw_info.git_maj, gw_info.git_min, gw_info.git_rev, gw_info.git_additional,
                        str::from_utf8(&gw_info.host_str[..gw_info.host_len as usize]).unwrap(),
                        str::from_utf8(&gw_info.date_str[..gw_info.date_len as usize]).unwrap()
                    )
                } else {
                    format!("v{}.{}.{}+{}\ncommit: g{:x}\n@{}\n{}",
                        gw_info.git_maj, gw_info.git_min, gw_info.git_rev, gw_info.git_additional,
                        gw_info.git_commit,
                        str::from_utf8(&gw_info.host_str[..gw_info.host_len as usize]).unwrap(),
                        str::from_utf8(&gw_info.date_str[..gw_info.date_len as usize]).unwrap()
                    )
                };

                let mut skip_confirmation = false;
                match modals.get_radiobutton(prompt) {
                    Ok(response) => {
                        if response == t!("rootkeys.gwup.short", xous::LANG) {
                            modals.show_notification(info.as_str()).expect("modals error");
                        } else if response == t!("rootkeys.gwup.details", xous::LANG) {
                            modals.show_notification(info.as_str()).expect("modals error");
                            let gw_info = keys.fetch_gw_metadata(GatewareRegion::Staging);
                            // truncate the message to better fit in the rendering box
                            let info_len = if gw_info.log_len > 256 { 256 } else {gw_info.log_len};
                            let info = format!("{}", str::from_utf8(&gw_info.log_str[..info_len as usize]).unwrap());
                            modals.show_notification(info.as_str()).expect("modals error");
                            // truncate the message to better fit in the rendering box
                            let status_len = if gw_info.status_len > 256 { 256 } else {gw_info.status_len};
                            let info = format!("{}", str::from_utf8(&gw_info.status_str[..status_len as usize]).unwrap());
                            modals.show_notification(info.as_str()).expect("modals error");
                        } else {
                            skip_confirmation = true;
                        }
                    }
                    _ => {log::error!("get_radiobutton failed"); continue;}
                }
                if !skip_confirmation {
                    modals.add_list_item(t!("rootkeys.gwup.yes", xous::LANG)).expect("modals error");
                    modals.add_list_item(t!("rootkeys.gwup.no", xous::LANG)).expect("modals error");
                    match modals.get_radiobutton(t!("rootkeys.gwup.proceed_confirm", xous::LANG)) {
                        Ok(response) => {
                            if response == t!("rootkeys.gwup.no", xous::LANG) {
                                continue;
                            } if response != t!("rootkeys.gwup.yes", xous::LANG) {
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
                }

                // here, we always set the password policy to "keep until suspend". Maybe we want to change this, maybe we
                // want to refer to the PDDB to do something different, but in retrospect asking this question to users is dumb
                // and annoying.

                if keys.is_pcache_update_password_valid() {
                    // indicate that there should be no change to the policy
                    let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", xous::LANG));
                    let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                    buf.send(main_cid, Opcode::UxUpdateGwRun.to_u32().unwrap())
                    .map(|_| ()).expect("couldn't send action message");
                } else {
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::UxUpdateGwPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action)),
                        Some(t!("rootkeys.get_update_password", xous::LANG)), false,
                        None, true, None
                    );
                    #[cfg(feature="tts")]
                    tts.tts_blocking(t!("rootkeys.get_update_password", xous::LANG)).unwrap();
                    rootkeys_modal.activate();
                }
            }
            Some(Opcode::UxUpdateGwPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                // indicate that there should be no change to the policy
                let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", xous::LANG));
                let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                buf.send(main_cid, Opcode::UxUpdateGwRun.to_u32().unwrap())
                .map(|_| ()).expect("couldn't send action message");
            }
            Some(Opcode::UxUpdateGwRun) => {
                // this is a bit of legacy code to handle a return from a menu that would set our password policy.
                // for now, this is short-circuited because every branch that leads into here selects "policy_suspend",
                // which is a 99% correct but 100% more user-friendly policy
                #[cfg(feature = "policy-menu")]
                {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if payload.as_str() == t!("rootkeys.policy_keep", xous::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                    } else if payload.as_str() == t!("rootkeys.policy_suspend", xous::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    } else if payload.as_str() == "no change" {
                        // don't change the policy
                    } else {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                    }
                }
                keys.set_ux_password_type(None);

                let result = keys.do_gateware_update(&mut rootkeys_modal, main_cid, false);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals.show_notification(t!("rootkeys.gwup.finished", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals.show_notification(t!("rootkeys.init.fail_alignment", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals.show_notification(t!("rootkeys.init.fail_key", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals.show_notification(t!("rootkeys.init.fail_verify", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals.show_notification(t!("rootkeys.init.fail_burn", xous::LANG)).expect("modals error");
                    }
                }
            }
            Some(Opcode::UxSelfSignXous) => {
                if keys.is_pcache_update_password_valid() {
                    // set a default policy
                    let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", xous::LANG));
                    let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                    buf.send(main_cid, Opcode::UxSignXousRun.to_u32().unwrap())
                    .map(|_| ()).expect("couldn't send action message");
                } else {
                    keys.set_ux_password_type(Some(PasswordType::Update));
                    password_action.set_action_opcode(Opcode::UxSignXousPasswordReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action)),
                        Some(t!("rootkeys.get_signing_password", xous::LANG)), false,
                        None, true, None
                    );
                    #[cfg(feature="tts")]
                    tts.tts_blocking(t!("rootkeys.get_signing_password", xous::LANG)).unwrap();
                    rootkeys_modal.activate();
                }
            },
            Some(Opcode::UxSignXousPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                let payload = gam::RadioButtonPayload::new(t!("rootkeys.policy_suspend", xous::LANG));
                let buf = Buffer::into_buf(payload).expect("couldn't convert message to payload");
                buf.send(main_cid, Opcode::UxSignXousRun.to_u32().unwrap())
                .map(|_| ()).expect("couldn't send action message");
            },
            Some(Opcode::UxSignXousRun) => {
                #[cfg(feature = "policy-menu")]
                {// legacy code to set policy, if it were to be inserted in the flow
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if payload.as_str() == t!("rootkeys.policy_keep", xous::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                    } else if payload.as_str() == t!("rootkeys.policy_suspend", xous::LANG) {
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    } else if payload.as_str() == "no change" {
                        // don't change the policy
                    } else {
                        keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                    }
                }
                keys.set_ux_password_type(None);

                let result = keys.do_sign_xous(&mut rootkeys_modal, main_cid);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals.show_notification(t!("rootkeys.signxous.finished", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals.show_notification(t!("rootkeys.init.fail_alignment", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals.show_notification(t!("rootkeys.init.fail_key", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals.show_notification(t!("rootkeys.init.fail_verify", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals.show_notification(t!("rootkeys.init.fail_burn", xous::LANG)).expect("modals error");
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
                    //password_action.set_action_opcode(Opcode::UxAesPasswordPolicy.to_u32().unwrap()); // skip policy question. it's annoying.
                    password_action.set_action_opcode(Opcode::UxAesEnsureReturn.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::TextEntry(password_action)),
                        Some(t!("rootkeys.get_login_password", xous::LANG)), false,
                        None, true, None
                    );
                    #[cfg(feature="tts")]
                    tts.tts_blocking(t!("rootkeys.get_login_password", xous::LANG)).unwrap();
                    rootkeys_modal.activate();
                    // note that the scalar is *not* yet returned, it will be returned by the opcode called by the password assurance
                } else {
                    // insert other indices, as we come to have them in else-ifs
                    // note that there needs to be a way to keep the ensured password in sync with the
                    // actual key index (if multiple passwords are used/required). For now, because there is only
                    // one password, we can use is_pcache_boot_password_valid() to sync that up; but as we add
                    // more keys with more passwords, this policy may need to become markedly more complicated!

                    // otherwise, an invalid password request
                    modals.show_notification(t!("rootkeys.bad_password_request", xous::LANG)).expect("modals error");

                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::UxAesPasswordPolicy) => { // this is bypassed, it's not useful. You basically always only want to retain the password until sleep.
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                let mut confirm_radiobox = gam::modal::RadioButtons::new(
                    main_cid,
                    Opcode::UxAesEnsureReturn.to_u32().unwrap()
                );
                confirm_radiobox.is_password = true;
                confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_suspend", xous::LANG)));
                // confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_clear", xous::LANG))); // this policy makes no sense in the use case of the key
                confirm_radiobox.add_item(ItemName::new(t!("rootkeys.policy_keep", xous::LANG)));
                rootkeys_modal.modify(
                    Some(ActionType::RadioButtons(confirm_radiobox)),
                    Some(t!("rootkeys.policy_request", xous::LANG)), false,
                    None, true, None);
                #[cfg(feature="tts")]
                tts.tts_blocking(t!("rootkeys.policy_request", xous::LANG)).unwrap();
                rootkeys_modal.activate();
            },
            Some(Opcode::UxAesEnsureReturn) => {
                if let Some(sender) = aes_sender.take() {
                    xous::return_scalar(sender, 1).unwrap();
                    #[cfg(feature = "policy-menu")]
                    { // in case we want to bring back the policy check
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let payload = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                        if payload.as_str() == t!("rootkeys.policy_keep", xous::LANG) {
                            keys.update_policy(Some(PasswordRetentionPolicy::AlwaysKeep));
                        } else if payload.as_str() == t!("rootkeys.policy_suspend", xous::LANG) {
                            keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                        } else if payload.as_str() == "no change" {
                            // don't change the policy
                        } else {
                            keys.update_policy(Some(PasswordRetentionPolicy::AlwaysPurge)); // default to the most paranoid level
                        }
                    }
                    {
                        let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                        keys.hash_and_save_password(plaintext_pw.as_str());
                        plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                        buf.volatile_clear();

                        // this is a reasonable default policy -- don't bother the user to answer this question all the time.
                        keys.update_policy(Some(PasswordRetentionPolicy::EraseOnSuspend));
                    }

                    keys.set_ux_password_type(None);
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                    log::warn!("UxAesEnsureReturn detected a fat-finger event. Ignoring.");
                }
            }
            Some(Opcode::AesOracle) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                // as_flat saves a copy step, but we have to deserialize some enums manually
                let mut aes_op = buffer.to_original::<AesOp, _>().unwrap();
                let op = match aes_op.aes_op { // seems stupid, but we have to do this because we want to have zeroize on the AesOp record, and it means we can't have Copy on this.
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
            },
            Some(Opcode::AesKwp) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut kwp = buffer.to_original::<KeyWrapper, _>().unwrap();
                keys.kwp_op(&mut kwp);
                buffer.replace(kwp).unwrap();
            }

            Some(Opcode::BbramProvision) => {
                modals.show_notification(t!("rootkeys.bbram.confirm", xous::LANG)).expect("modals error");
                let console_input = gam::modal::ConsoleInput::new(
                    main_cid,
                    Opcode::UxBbramCheckReturn.to_u32().unwrap()
                );
                rootkeys_modal.modify(
                    Some(ActionType::ConsoleInput(console_input)),
                    Some(t!("rootkeys.console_input", xous::LANG)), false,
                    None, true, None);
                #[cfg(feature="tts")]
                tts.tts_blocking(t!("rootkeys.console_input", xous::LANG)).unwrap();
                rootkeys_modal.activate();
                log::info!("{}check_conn", CONSOLE_SENTINEL);
            }
            Some(Opcode::UxBbramCheckReturn) => {
                let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let console_text = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();
                log::info!("got console text:{}", console_text.as_str());
                if console_text.as_str().starts_with("HELPER_OK") {
                    log::info!("got '{}', moving on", console_text.as_str());
                    // proceed
                    if keys.is_pcache_update_password_valid() || !keys.is_initialized() {
                        send_message(main_cid,
                            xous::Message::new_scalar(Opcode::UxBbramRun.to_usize().unwrap(), 0, 0, 0, 0)
                        ).unwrap();
                    } else {
                        keys.set_ux_password_type(Some(PasswordType::Update));
                        password_action.set_action_opcode(Opcode::UxBbramPasswordReturn.to_u32().unwrap());
                        rootkeys_modal.modify(
                            Some(ActionType::TextEntry(password_action)),
                            Some(t!("rootkeys.get_signing_password", xous::LANG)), false,
                            None, true, None
                        );
                        #[cfg(feature="tts")]
                        tts.tts_blocking(t!("rootkeys.get_signing_password", xous::LANG)).unwrap();
                        rootkeys_modal.activate();
                    }
                } else {
                    modals.show_notification(t!("rootkeys.bbram.no_helper", xous::LANG)).expect("modals error");
                    continue;
                }
            }
            Some(Opcode::UxBbramPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
                send_message(main_cid,
                    xous::Message::new_scalar(Opcode::UxBbramRun.to_usize().unwrap(), 0, 0, 0, 0)
                ).unwrap();
            }
            Some(Opcode::UxBbramRun) => {
                keys.set_ux_password_type(None);
                let result = keys.do_gateware_update(&mut rootkeys_modal, main_cid, true);
                // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
                rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                match result {
                    Ok(_) => {
                        modals.show_notification(t!("rootkeys.bbram.finished", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::AlignmentError) => {
                        modals.show_notification(t!("rootkeys.init.fail_alignment", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::KeyError) => {
                        // probably a bad password, purge it, so the user can try again
                        keys.purge_password(PasswordType::Update);
                        modals.show_notification(t!("rootkeys.init.fail_key", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::IntegrityError) => {
                        modals.show_notification(t!("rootkeys.init.fail_verify", xous::LANG)).expect("modals error");
                    }
                    Err(RootkeyResult::FlashError) => {
                        modals.show_notification(t!("rootkeys.init.fail_burn", xous::LANG)).expect("modals error");
                    }
                }
            }

            Some(Opcode::CheckGatewareSignature) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if keys.is_initialized() {
                    if keys.verify_gateware_self_signature() {
                        xous::return_scalar(msg.sender, 1).expect("couldn't send return value");
                    } else {
                        xous::return_scalar(msg.sender, 0).expect("couldn't send return value");
                    }
                } else {
                    xous::return_scalar(msg.sender, 2).expect("couldn't send return value");
                }
            }),
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
                    send_message(main_cid,
                        xous::Message::new_scalar(action, 0, 0, 0, 0)
                    ).unwrap();
                }
                policy_followup_action = None;
            }),
            Some(Opcode::UxGutter) => {
                // an intentional NOP for UX actions that require a destintation but need no action
            },



            // boilerplate Ux handlers
            Some(Opcode::ModalRedraw) => {
                rootkeys_modal.redraw();
            },
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
                break
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
