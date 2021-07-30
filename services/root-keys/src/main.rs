#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use xous::{CID, msg_scalar_unpack, send_message, Message, msg_blocking_scalar_unpack};
use xous_ipc::{String, Buffer};

use num_traits::*;

use gam::modal::*;
use gam::menu::*;

use locales::t;

use core::fmt::Write;

#[cfg(target_os = "none")]
mod implementation;
#[cfg(target_os = "none")]
use implementation::*;

#[cfg(target_os = "none")]
mod bcrypt;

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use crate::ROOTKEY_MODAL_NAME;
    use crate::PasswordRetentionPolicy;
    use crate::PasswordType;

    pub struct RootKeys {
    }

    impl RootKeys {
        pub fn new(xns: &xous_names::XousNames) -> RootKeys {
            RootKeys {
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
        pub fn try_init_keys(&mut self) {
        }
        pub fn set_ux_password_type(&mut self, _cur_type: PasswordType) {
        }
        pub fn is_initialized(&self) -> bool {false}
        pub fn setup_key_init(&mut self) {}
    }
}

// enumerate the possible password types dealt with by this manager
// the discriminant is used to every-so-slightly change the salt going into bcrypt
// I don't think it hurts; but it also prevents an off-the-shelf "hashcat" run from
// being used to brute force all the passwords in a single go.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PasswordType {
    Boot = 1,
    Update = 2,
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::RootKeys;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
       Connections allowed to the keys server:
          1. Shellchat (to originate update test requests)
          2. Password entry UX thread
          3. Key purge timer
          4. Main menu -> trigger initialization
          5. (future) PDDB
    */
    let keys_sid = xns.register_name(api::SERVER_NAME_KEYS, Some(4)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", keys_sid);

    let mut keys = RootKeys::new(&xns);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let main_cid = xous::connect(keys_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, main_cid).expect("couldn't create suspend/resume object");

    // create a policy menu object
    let mut policy_menu = gam::menu::Menu::new(crate::ROOTKEY_MENU_NAME);
    policy_menu.add_item(MenuItem {
        name: String::<64>::from_str(t!("rootkeys.policy_keep", xous::LANG)),
        action_conn: main_cid,
        action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysKeep.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.add_item(MenuItem {
        name: String::<64>::from_str(t!("rootkeys.policy_suspend", xous::LANG)),
        action_conn: main_cid,
        action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::EraseOnSuspend.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.add_item(MenuItem {
        name: String::<64>::from_str(t!("rootkeys.policy_clear", xous::LANG)),
        action_conn: main_cid,
        action_opcode: Opcode::UxPolicyReturn.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysPurge.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.spawn_helper(keys_sid, policy_menu.sid,
        Opcode::MenuRedraw.to_u32().unwrap(),
        Opcode::MenuKeys.to_u32().unwrap(),
        Opcode::MenuDrop.to_u32().unwrap());

    let password_action = TextEntry {
        is_password: true,
        visibility: TextEntryVisibility::LastChars,
        action_conn: main_cid,
        action_opcode: Opcode::UxPasswordReturn.to_u32().unwrap(),
        action_payload: TextEntryPayload::new(),
        validator: None,
    };
    let mut dismiss_modal_action = Notification::new(main_cid, Opcode::UxGutter.to_u32().unwrap());
    dismiss_modal_action.set_is_password(true);

    let mut rootkeys_modal = Modal::new(
        crate::api::ROOTKEY_MODAL_NAME,
        ActionType::TextEntry(password_action),
        Some(t!("rootpass.top", xous::LANG)),
        None,
        GlyphStyle::Small,
        4
    );
    rootkeys_modal.spawn_helper(keys_sid, rootkeys_modal.sid,
        Opcode::ModalRedraw.to_u32().unwrap(),
        Opcode::ModalKeys.to_u32().unwrap(),
        Opcode::ModalDrop.to_u32().unwrap(),
    );

    let mut cur_plaintext = String::<256>::new();
    loop {
        let msg = xous::receive_message(keys_sid).unwrap();
        log::trace!("message: {:?}", msg);
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

            // UX flow opcodes
            Some(Opcode::UxTryInitKeys) => msg_scalar_unpack!(msg, _, _, _, _, {
                // overall flow:
                //  - setup the init
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
                    dismiss_modal_action.set_action_opcode(Opcode::UxGutter.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::Notification(dismiss_modal_action)),
                        Some(t!("rootkeys.already_init", xous::LANG)), false,
                        None, true, None);
                } else {
                    dismiss_modal_action.set_action_opcode(Opcode::UxRequestBootPassword.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::Notification(dismiss_modal_action)),
                        Some(t!("rootkeys.setup", xous::LANG)), false,
                        None, true, None);
                }
                rootkeys_modal.activate();
            }),
            Some(Opcode::UxRequestBootPassword) => {
                keys.setup_key_init();
                keys.set_ux_password_type(PasswordType::Boot);
                rootkeys_modal.modify(
                    Some(ActionType::TextEntry(password_action)),
                    Some(t!("rootpass.top", xous::LANG)), false,
                    None, true, None
                );
                rootkeys_modal.activate();
            }
            Some(Opcode::UxPasswordReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                // always explicitly request a policy decision for rootkeys
                policy_menu.activate();
            },
            Some(Opcode::UxPolicyReturn) => msg_scalar_unpack!(msg, policy_code, _, _, _, {
                keys.update_policy(FromPrimitive::from_usize(policy_code));
            }),
            /*
            Some(Opcode::TestUx) => msg_scalar_unpack!(msg, arg, _, _, _, {
                log::debug!("activating password modal");
                keys.set_ux_password_type(PasswordType::Boot);
                rootkeys_modal.activate();
                //policy_menu.activate();
            }),*/
            Some(Opcode::UxGutter) => {
                // an intentional NOP for UX actions that require a destintation but need no action
            },



            // boilerplate Ux handlers
            Some(Opcode::MenuRedraw) => {
                policy_menu.redraw();
            },
            Some(Opcode::MenuKeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k2 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k3 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k4 as u32) {a} else {'\u{0000}'},
                ];
                policy_menu.key_event(keys);
            }),
            Some(Opcode::MenuDrop) => {
                panic!("Menu handler for rootkeys quit unexpectedly");
            },
            Some(Opcode::ModalRedraw) => {
                rootkeys_modal.redraw();
            },
            Some(Opcode::ModalKeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k2 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k3 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k4 as u32) {a} else {'\u{0000}'},
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
