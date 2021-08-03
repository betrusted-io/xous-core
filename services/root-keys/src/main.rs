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
    use gam::modal::{Modal, Slider};

    pub struct RootKeys {
        password_type: Option<PasswordType>,
    }

    impl RootKeys {
        pub fn new(xns: &xous_names::XousNames) -> RootKeys {
            RootKeys {
                password_type: None,
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
        pub fn is_initialized(&self) -> bool {false}
        pub fn setup_key_init(&mut self) {}
        pub fn do_key_init(&mut self,progress_modal: &mut Modal, progress_action: &mut Slider) -> bool {
            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
            for i in 1..10 {
                log::info!("fake progress: {}", i * 10);
                progress_action.set_state(i);
                progress_modal.modify(
                    Some(gam::modal::ActionType::Slider(*progress_action)),
                    None, false, None, false, None);
                progress_modal.redraw(); // stage the modal box pixels to the back buffer
                progress_modal.gam.redraw().expect("couldn't cause back buffer to be sent to the screen");
                xous::yield_slice(); // this gives time for the GAM to do the sending
                ticktimer.sleep_ms(2000).unwrap();
            }
            true
        }
        pub fn get_ux_password_type(&self) -> Option<PasswordType> {self.password_type}
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

    let mut password_action = TextEntry {
        is_password: true,
        visibility: TextEntryVisibility::LastChars,
        action_conn: main_cid,
        action_opcode: Opcode::UxInitPasswordReturn.to_u32().unwrap(),
        action_payload: TextEntryPayload::new(),
        validator: None,
    };
    let mut dismiss_modal_action = Notification::new(main_cid, Opcode::UxGutter.to_u32().unwrap());
    dismiss_modal_action.set_is_password(true);
    let mut progress_action = Slider::new(main_cid, Opcode::UxGutter.to_u32().unwrap(),
        0, 100, 10, Some("%"), 0, true, true
    );
    progress_action.set_is_password(true);

    let mut rootkeys_modal = Modal::new(
        crate::api::ROOTKEY_MODAL_NAME,
        ActionType::TextEntry(password_action),
        Some(t!("rootkeys.bootpass", xous::LANG)),
        None,
        GlyphStyle::Small,
        4
    );
    rootkeys_modal.spawn_helper(keys_sid, rootkeys_modal.sid,
        Opcode::ModalRedraw.to_u32().unwrap(),
        Opcode::ModalKeys.to_u32().unwrap(),
        Opcode::ModalDrop.to_u32().unwrap(),
    );

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
                    keys.set_ux_password_type(None);
                } else {
                    dismiss_modal_action.set_action_opcode(Opcode::UxInitRequestPassword.to_u32().unwrap());
                    rootkeys_modal.modify(
                        Some(ActionType::Notification(dismiss_modal_action)),
                        Some(t!("rootkeys.setup", xous::LANG)), false,
                        None, true, None);
                    // setup_key_init() prepares the salt and other items necessary to receive a password safely
                    keys.setup_key_init();
                    // request the boot password first
                    keys.set_ux_password_type(Some(PasswordType::Boot));
                }
                rootkeys_modal.activate();
            }),
            Some(Opcode::UxInitRequestPassword) => {
                password_action.set_action_opcode(Opcode::UxInitPasswordReturn.to_u32().unwrap());
                if let Some(pwt) = keys.get_ux_password_type() {
                    match pwt {
                        PasswordType::Boot => {
                            rootkeys_modal.modify(
                                Some(ActionType::TextEntry(password_action)),
                                Some(t!("rootkeys.bootpass", xous::LANG)), false,
                                None, true, None
                            );
                        }
                        PasswordType::Update => {
                            rootkeys_modal.modify(
                                Some(ActionType::TextEntry(password_action)),
                                Some(t!("rootkeys.updatepass", xous::LANG)), false,
                                None, true, None
                            );
                        }
                    }
                    rootkeys_modal.activate();
                } else {
                    log::error!("init password ux request without a password type requested!");
                }
            }
            Some(Opcode::UxInitPasswordReturn) => {
                // assume:
                //   - setup_key_init has also been called (exactly once, before anything happens)
                //   - set_ux_password_type has been called already
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut plaintext_pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                keys.hash_and_save_password(plaintext_pw.as_str());
                plaintext_pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();

                if let Some(pwt) = keys.get_ux_password_type() {
                    match pwt {
                        PasswordType::Boot => {
                            // now grab the update password
                            keys.set_ux_password_type(Some(PasswordType::Update));
                            send_message(main_cid,
                                xous::Message::new_scalar(Opcode::UxInitRequestPassword.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't initiate dialog box");
                        }
                        PasswordType::Update => {
                            keys.set_ux_password_type(None);
                            // now show the init wait note...
                            rootkeys_modal.modify(
                                Some(ActionType::Slider(progress_action)),
                                Some(t!("rootkeys.setup_wait", xous::LANG)), false,
                                None, true, None);
                            rootkeys_modal.activate();

                            xous::yield_slice(); // give some time to the GAM to render

                            // this routine will update the rootkeys_modal with the current Ux state
                            let success = keys.do_key_init(&mut rootkeys_modal, &mut progress_action);

                            // the stop emoji, when sent to the slider action bar in progress mode, will cause it to close and relinquish focus
                            rootkeys_modal.key_event(['🛑', '\u{0000}', '\u{0000}', '\u{0000}']);

                            // at this point, we may want to pop up a modal indicating we are going to reboot?
                            // we also need to include a command that does the reboot.
                            if success {
                                // do a reboot
                            }
                        }
                    }
                } else {
                    log::error!("invalid UX state -- someone called init password return, but no password type was set!");
                }
            },
            /*
            Some(Opcode::TestUx) => msg_scalar_unpack!(msg, arg, _, _, _, {
                log::debug!("activating password modal");
                keys.set_ux_password_type(PasswordType::Boot);
                rootkeys_modal.activate();
                //policy_menu.activate();
            }),*/
            Some(Opcode::UxGetPolicy) => {
                policy_menu.activate();
            }
            Some(Opcode::UxPolicyReturn) => msg_scalar_unpack!(msg, policy_code, _, _, _, {
                keys.update_policy(FromPrimitive::from_usize(policy_code));
            }),
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
