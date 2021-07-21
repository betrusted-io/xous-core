#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use xous::{CID, msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message};
use num_traits::*;

pub use cipher::{self, BlockCipher, BlockDecrypt, BlockEncrypt, consts::U16};

/// 128-bit AES block
pub type Block = cipher::generic_array::GenericArray<u8, cipher::consts::U16>;

/// 16 x 128-bit AES blocks to be processed in bulk
pub type ParBlocks = cipher::generic_array::GenericArray<Block, cipher::consts::U16>;

pub enum RootPasswordType {
    Update,
    Boot,
}
pub enum ImageType {
    All,
    Gateware,
    Loader,
    Kernel,
}

pub struct RootKeys {
    conn: CID,
    progress_sid: Option<xous::SID>,
    // index of the key to use for the next encrypt/decrypt ops
    key_index: Option<u8>,
}
impl RootKeys {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_KEYS).expect("Can't connect to Keys server");
        Ok(RootKeys {
            conn,
            progress_sid: None,
            key_index: None,
        })
    }
    fn ensure_progress_server(&mut self) -> xous::SID {
        if let Some(sid) = self.progress_sid {
            sid
        } else {
            let sid = xous::create_server().unwrap();
            self.progress_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(progress_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            sid
        }
    }

    /// this function causes the staging gateware to be provisioned with a copy of our keys,
    /// while being encrypted to the AES key indicated inside the KEYROM
    pub fn provision_and_encrypt_staging_gateware(&mut self, progress: Option<fn(ProgressReport)>) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// this function causes the staging gateware to be copied to the boot gateware region.
    /// it can report progress of the operation by sending a message to an optional ScalarHook
    pub fn copy_staging_to_boot_gateware(&mut self, progress: Option<fn(ProgressReport)>) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// this function takes a boot gateware that has a "null" key (all zeros) and:
    /// 0. confirms that JTAG is accessible by reading out the ID, and that the eFuse is writeable, and all 0's
    /// 1. prompts of the update password, and confirms that the existing efuse key decrypts to 0. if not -- we were already fused, abort.
    /// 2. creates a new eFuse key using the TRNG
    /// 3. inserts a revised KEYROM into the base image while copying & encrypting with the new key to the staging area
    /// 4. verifies the xilinx-hmac on the staged image
    /// 5. copies the staged image into the boot area
    /// 6. burns the eFuse key using JTAG-local commands
    /// 7. suspends the device with auto-resume so that the new gateware is in effect
    /// 8. reads back the eFuse key from the KEYROM to confirm everything went as planned, compares to previously computed result
    /// 9. clears the eFuse key from RAM.
    pub fn seal_boot_gateware(&mut self, progress: Option<fn(ProgressReport)>) -> Result<(), xous::Error> {
        unimplemented!();
    }

    /// checks to see if the KEYROM entries are 0, and if so, generates keys. In the process of doing so, the user will be
    /// prompted to enter passwords. It also automatically calls self_sign() -- presumably, if you were comfortable enough to
    /// use this firmware to make your keys, you also trusted it.
    /// it will then update the bitstream with your keys.
    /// if the KEYROM entries are not 0, it will abort without any user prompt, but with an error code.
    pub fn try_init_keys(&mut self, progress: Option<fn(ProgressReport)>) -> Result<(), xous::Error> {
        if let Some(cb) = progress {
            unsafe {
                PROGRESS_CB = Some(cb);
            }
            let sid = self.ensure_progress_server().to_array();
            send_message(self.conn,
                Message::new_scalar(Opcode::TryInitKeysWithProgress.to_usize().unwrap(),
                sid[0] as usize, sid[1] as usize, sid[2] as usize, sid[3] as usize)
            ).map(|_| ())
        } else {
            unsafe {
                PROGRESS_CB = None;
            }
            send_message(self.conn,
                Message::new_scalar(Opcode::TryInitKeys.to_usize().unwrap(),
                0, 0, 0, 0)
            ).map(|_| ())
        }
    }

    /// this initiates an attempt to update passwords. User must unlock their device first, and can cancel out if not expected.
    pub fn try_update_password(&mut self, which: RootPasswordType) -> Result<(), xous::Error> {
        unimplemented!();
    }

    //// initiates a self-signing of the firmwares using the ed25519 private key stored in the enclave
    pub fn self_sign(&mut self, which: ImageType) -> Result<(), xous::Error> {
        unimplemented!();
    }

}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for RootKeys {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }

        // de-allocate our progress responder server, if it existed
        if let Some(sid) = self.progress_sid {
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_scalar(ProgressCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }
    }
}


impl BlockCipher for RootKeys {
    type BlockSize = U16;   // 128-bit cipher
    type ParBlocks = U16;   // 256-byte "chunk" if doing more than one block at a time, for better efficiency
}

impl BlockEncrypt for RootKeys {
    fn encrypt_block(&self, block: &mut Block) {
        // put data in a buf, send it to the server for encryption
    }
    fn encrypt_par_blocks(&self, blocks: &mut ParBlocks) {

    }
}

impl BlockDecrypt for RootKeys {
    fn decrypt_block(&self, block: &mut Block) {
        
    }
    fn decrypt_par_blocks(&self, blocks: &mut ParBlocks) {

    }
}

/// handles progress messages from root-key server, in the library user's process space.
static mut PROGRESS_CB: Option<fn(ProgressReport)> = None;
fn progress_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ProgressCallback::Update) => msg_scalar_unpack!(msg, current, total, finished, _, {
                let report = ProgressReport {
                    current_step: current as u32,
                    total_steps: total as u32,
                    finished: if finished != 0 {true} else {false}
                };
                unsafe {
                    if let Some (cb) = PROGRESS_CB {
                        cb(report)
                    }
                }
            }),
            Some(ProgressCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
