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
        pub fn set_plaintext_password(&mut self, pw: &str) {
            log::info!("got password plaintext: {}", pw);
        }
        pub fn try_init_keys(&mut self, _maybe_progress: Option<xous::SID>) {
        }
    }
}

// enumerate the possible password types dealt with by this manager
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PasswordType {
    Boot,
    Update,
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
          4. (future) PDDB
    */
    let keys_sid = xns.register_name(api::SERVER_NAME_KEYS, Some(3)).expect("can't register server");
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
        action_opcode: Opcode::PasswordPolicy.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysKeep.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.add_item(MenuItem {
        name: String::<64>::from_str(t!("rootkeys.policy_suspend", xous::LANG)),
        action_conn: main_cid,
        action_opcode: Opcode::PasswordPolicy.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::EraseOnSuspend.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.add_item(MenuItem {
        name: String::<64>::from_str(t!("rootkeys.policy_clear", xous::LANG)),
        action_conn: main_cid,
        action_opcode: Opcode::PasswordPolicy.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([PasswordRetentionPolicy::AlwaysPurge.to_u32().unwrap(), 0, 0, 0,]),
        close_on_select: true,
    });
    policy_menu.spawn_helper(keys_sid, policy_menu.sid,
        Opcode::MenuRedraw.to_u32().unwrap(),
        Opcode::MenuKeys.to_u32().unwrap(),
        Opcode::MenuDrop.to_u32().unwrap());

    //xous::create_thread_0(rootkeys_ux_thread).expect("couldn't start rootkeys UX thread");
    let password_action = gam::modal::TextEntry {
        is_password: true,
        visibility: gam::modal::TextEntryVisibility::LastChars,
        action_conn: main_cid,
        action_opcode: Opcode::PasswordModalEntry.to_u32().unwrap(),
        action_payload: TextEntryPayload::new(),
        validator: None,
    };
    let mut password_modal = gam::Modal::new(
        crate::api::ROOTKEY_MODAL_NAME,
        gam::ActionType::TextEntry(password_action),
        Some(t!("rootpass.top", xous::LANG)),
        None,
        GlyphStyle::Small,
        4
    );
    password_modal.spawn_helper(keys_sid, password_modal.sid,
        Opcode::ModalRedraw.to_u32().unwrap(),
        Opcode::ModalKeys.to_u32().unwrap(),
        Opcode::ModalDrop.to_u32().unwrap(),
    );

    let mut cur_plaintext = String::<256>::new();
    let mut cur_password_type: Option<PasswordType> = None;
    loop {
        let msg = xous::receive_message(keys_sid).unwrap();
        log::trace!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => msg_scalar_unpack!(msg, token, _, _, _, {
                keys.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                keys.resume();
            }),
            Some(Opcode::TryInitKeys) => msg_scalar_unpack!(msg, _, _, _, _, {
                keys.try_init_keys(None);
            }),
            Some(Opcode::TryInitKeysWithProgress) => msg_scalar_unpack!(msg, s0, s1, s2, s3, {
                let sid = xous::SID::from_u32(s0 as u32, s1 as u32, s2 as u32, s3 as u32);
                keys.try_init_keys(Some(sid));
            }),
            Some(Opcode::TestUx) => msg_scalar_unpack!(msg, arg, _, _, _, {
                log::debug!("activating password modal");
                password_modal.activate();
                //policy_menu.activate();
            }),
            Some(Opcode::PasswordModalEntry) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                if let Some(pw_type) = cur_password_type {
                    keys.hash_and_save_password(plaintext_pw.as_str(), pw_type);
                    plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                    buf.volatile_clear();

                    // now explicitly request a policy decision
                    xous::send_message(main_cid,
                        Message::new_scalar(Opcode::RaisePolicyMenu.to_usize().unwrap(), 0, 0, 0, 0)
                    ).expect("couldn't raise policy menu follow-up dialogue");
                } else {
                    plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                    buf.volatile_clear();
                    log::error!("got a password from our UX but didn't expect one. Ignoring it.")
                }
            },
            Some(Opcode::RaisePolicyMenu) => {
                xous::yield_slice();  // let any pending redraws happen before raising this menu
                log::debug!("raising policy menu");
                policy_menu.activate();
            }
            Some(Opcode::PasswordPolicy) => msg_scalar_unpack!(msg, policy_code, _, _, _, {
                if let Some(pw_type) = cur_password_type {
                    keys.update_policy(FromPrimitive::from_usize(policy_code), pw_type);
                } else {
                    log::error!("got a policy from our UX but didn't expect one. Ignoring.")
                }
            }),

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
                password_modal.redraw();
            },
            Some(Opcode::ModalKeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k2 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k3 as u32) {a} else {'\u{0000}'},
                    if let Some(a) = core::char::from_u32(k4 as u32) {a} else {'\u{0000}'},
                ];
                password_modal.key_event(keys);
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
