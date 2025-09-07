use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use num_traits::*;
use totp::PumpOp;
mod ux;
use ux::*;
mod itemcache;
use itemcache::*;
// mod actions;
// use actions::ActionOp;
// mod storage;
mod totp;

/*
Dev status & notes --

Aug 3 2025:

`cargo xtask baosec-emu` will launch the emulated UI. The aspect currently
in progress is the user interaction. Some firm conclusions so far are:

- There is only going to be a TOTP and a password flow. FIDO interactions
will be through modal pop-ups only that alert of the relying party (if FIDO2).
FIDO-1 will notify that this is a legacy transaction with no identifier.
- We will use the same PDDB record format as vault original
- At the moment all "actions" are commented out in this app, so it is just a shell
used to refine the UI flow. Once the UI flow is refined we will add in the
live functionality
- Likewise there is no HID handler loop or camera loop
- Menu interactions will happen by pressing the "home" key and will be
done with an explicitly coded menu routine. This is because we don't have
a GAM in this implementation to handle layering/compositing in this UI framework.

-[ ] harmonized TOTP & password interactions? It is not yet clear if we are
going to have two separate UI flows, or if we will share elements between the
two. I am inclined to harmonize the two, which means reworking the TOTP flow
to have the search entry at the bottom.
  -[x] TOTP interaction prototyped
  -[ ] password interaction prototyped
-[ ] implement "home" button press menu mode
-[ ] implement search by text entry - activated via both menu pop-up, and by
selecting >search prefix< on the bottom line of UI

There are three action loops that need to be implemented. These are not
necessarily listed in order of implementation:
  -[ ] implement QR code scanning loop
  -[ ] implement HID loop
  -[ ] implement "action" loop

-[ ] implement keystore
  -[ ] Consider the side channel resistance of the AES implementation in the keystore. We may want
      to use an AES API that explicitly wraps the SCE's masked AES implementation to reduce side channels,
      versus the Vex CPU core's AES implementation
  -[ ] PDDB should have a base 256-bit key to protect all entries, stored in key store
       This is equivalent to the "Backup key" in precursor
    -[ ] Base 256-bit key is derived from hash of 64kbits raw data scattered
    across the RRAM array. Locations are chosen to be diverse, with the goal of reducing
    voltage contrast due to parasitic leakage masking bits
    -[ ] 64kbit array is split into four 16kbit chunks, which are XOR'd together to create
    a 16kbit number. The four 16kbit chunks are read out 256-bits at a time, with an order
    determined by a random number on boot. The random ordering frustrates side channel attacks
    on read-out of the array. Thus the format is:
      - Kn, where n=[0..=3]
      - Each Kn is 16384 bits long, composed of 64 256-bit blocks: Bm = {B0, B1, ..Bm},
        where m=[0..=255]
      - The pre-hash key matter P is composed of K0 ^ K1 ^ K2 ^ K3, and the final key
        is SHA512/256(P)
      - P is derived specifically by visiting every one of KnBm in a random order and XOR'ing
      them together, such that:
        - The 16384-bit P is initialized to all 0's
        - Break P into 64 separate 256-bit long blocks. Fill each block as follows:
            - R is a 5-bit random number from the TRNG. It is rejection-sampled to generate a number
            Rk that is [0..=23] (i.e. any number 24-31 is rejected and the TRNG is run again).
            - Rk is turned into a permutation using the factoradic system:
            - Derive factoradic digits [d₁, d₂, d₃, d₄] where:
                - d1=k÷3!d1​=k÷3!
                - d2=(kmod  3!)÷2!d2​=(kmod3!)÷2!
                - d3=(kmod  2!)÷1!d3​=(kmod2!)÷1!
                - d4=0d4​=0 (there should only be one element left at this point)
            - Starting with the ordered list [A,B,C,D], at step i,
                pick the element at position dᵢ and remove it.
            - [A, B, C, D] represent visiting one of bank K0..=3
            - Fetching 256 bits from the respective bank and XOR it into the block position in P
        - The final backup key is computed as SHA512/256(DS | P), where DS is a domain separator consisting
          of 8 bits of 0's (8 bits is chosen simply because it's convenient, not to imply we could have only
          256 domains - you can always add more bits at the top of the hash).
        - Other keys may be derived by changing the domain separator.
  -[ ] The PIN may be optionally set to one of three values:
    -[ ] nothing (implemented as all 0's in the cryptographic key)
    -[ ] a passcode (4-8 digits entered via keyboard)
      -[ ] Configurable self-destruct after # of incorrect PIN guesses. Set at 8 by default.
      -[ ] Each incorrect guess has a 15 second timeout
    -[ ] a QR code (generated using browser & managed by user, 128 bits entropy)
  -[ ] Remember that re-pinning should be a low-cost operation. As such the PIN is used
   to protect a wrapped key that actually encrypts the PDDB, and not be the key itself.
  -[ ] One deniable basis at a time is allowed. It is mounted using using only:
    -[ ] a PIN (4-8 digits entered via keyboard)
    -[ ] a QR code (generated using browser & managed by user)
-[ ] implement backups
  -[ ] PIN confirmation required to set system into backup mode
  -[ ] Backup key is displayed. This is 256-bit key, displayed as QR code or Bip-39.
  -[ ] Hash & integrity blocks computed, stored
  -[ ] Backup data presented as .bin file on mass storage device
  -[ ] Device stays in backup state until user clears screen; exit via reboot

  */

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// Redraw the screen
    Redraw = 0,
    KeyPress,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Totp,
    Password,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct SelectedEntry {
    pub key_guid: String,
    pub description: String,
    pub mode: VaultMode,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Vault2 PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_VAULT2, None).expect("can't register server");
    let conn = xous::connect(sid).unwrap();

    // global shared state
    let mode = Arc::new(Mutex::new(VaultMode::Totp));
    let allow_totp_rendering = Arc::new(AtomicBool::new(true));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    // Protects access to the openSK PDDB entries from simultaneous readout on the UX while OpenSK is updating
    let opensk_mutex = Arc::new(Mutex::new(0));

    let mut vault_ui = VaultUi::new(&xns, conn, item_lists.clone(), mode.clone());

    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(mode.clone(), pump_sid, conn, allow_totp_rendering.clone());
    let pump_conn = xous::connect(pump_sid).unwrap();

    // respond to keyboard events
    let kbd = cramium_api::keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(SERVER_NAME_VAULT2, VaultOp::KeyPress.to_u32().unwrap() as usize);

    // spawn the actions server. This is responsible for grooming the UX elements. It
    // has to be in its own thread because it uses blocking modal calls that would cause
    // redraws of the background list to block/fail.
    let actions_sid = xous::create_server().unwrap();
    /*
    let _ = thread::spawn({
        let main_conn = conn.clone();
        let sid = actions_sid.clone();
        let mode = mode.clone();
        let item_lists = item_lists.clone();
        let action_active = action_active.clone();
        let opensk_mutex = opensk_mutex.clone();
        move || {
            let mut manager =
                crate::actions::ActionManager::new(main_conn, mode, item_lists, action_active, opensk_mutex);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew(); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuDeleteStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_delete(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuEditStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_edit(entry); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the
                        // underlying UX
                        manager.activate();
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateOneItem) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.update_db_entry(entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        // the password DBs are now not shared between modes, so no need to re-retrieve it.
                        if manager.is_db_empty() {
                            manager.retrieve_db();
                        }
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::ReloadDb) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                    #[cfg(feature = "vault-testing")]
                    Some(ActionOp::GenerateTests) => {
                        manager.populate_tests();
                        manager.retrieve_db();
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
    */
    let actions_conn = xous::connect(actions_sid).unwrap();
    // kickstart the pumper
    xous::send_message(pump_conn, xous::Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't start the pumper");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
                log::debug!("Got redraw");
                vault_ui.redraw();
            }
            Some(VaultOp::KeyPress) => xous::msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                log::info!("key {}", k);
                match k {
                    '↓' => {
                        vault_ui.nav(NavDir::Down);
                    }
                    '↑' => {
                        vault_ui.nav(NavDir::Up);
                    }
                    '←' => {
                        vault_ui.nav(NavDir::PageUp);
                    }
                    '→' => {
                        vault_ui.nav(NavDir::PageDown);
                    }
                    _ => {
                        log::info!("unhandled key {}", k);
                    }
                }
                vault_ui.redraw();
            }),
            _ => {
                log::error!("Got unknown message");
            }
        }
    }
}
