#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub mod cursor;
mod fontmap;
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
pub mod menu;
pub mod minigfx;
pub mod platform;
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
pub mod widgets;
#[cfg(feature = "std")]
pub mod wordwrap;
#[cfg(feature = "std")]
pub const SYSTEM_STYLE: blitstr2::GlyphStyle = blitstr2::GlyphStyle::Tall;
pub mod bitmaps;
#[cfg(feature = "std")]
pub mod service;

#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
use num_traits::*;
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
use xous::Message;
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
use xous_ipc::Buffer;

// common message forwarding infrastructure used by Menus, Modals, etc...
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
struct MsgForwarder {
    pub public_sid: [u32; 4],
    pub private_sid: [u32; 4],
    pub redraw_op: u32,
    pub rawkeys_op: u32,
    pub drop_op: u32,
}
/// this is a simple server that forwards incoming messages from a generic
/// "modal" interface to the internal private server. It keeps the GAM from being
/// able to tinker with the internal mechanics of the larger server that owns the
/// dialog box.
#[cfg(all(feature = "std", any(feature = "board-baosec", feature = "hosted-baosec")))]
pub(crate) fn forwarding_thread(addr: usize, size: usize, offset: usize) {
    let buf = unsafe { Buffer::from_raw_parts(addr, size, offset) };
    let forwarding_config = buf.to_original::<MsgForwarder, _>().unwrap();
    let private_conn = xous::connect(xous::SID::from_array(forwarding_config.private_sid))
        .expect("couldn't connect to the private server");

    log::trace!("modal forwarding server started");
    loop {
        let msg = xous::receive_message(xous::SID::from_array(forwarding_config.public_sid)).unwrap();
        log::trace!("modal forwarding server got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(crate::widgets::modal::ModalOpcode::Redraw) => {
                xous::send_message(
                    private_conn,
                    Message::new_scalar(forwarding_config.redraw_op as usize, 0, 0, 0, 0),
                )
                .expect("couldn't forward redraw message");
            }
            Some(crate::widgets::modal::ModalOpcode::Rawkeys) => {
                xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                    xous::send_message(
                        private_conn,
                        Message::new_scalar(forwarding_config.rawkeys_op as usize, k1, k2, k3, k4),
                    )
                    .expect("couldn't forard rawkeys message");
                })
            }
            Some(crate::widgets::modal::ModalOpcode::Quit) => {
                xous::send_message(
                    private_conn,
                    Message::new_scalar(forwarding_config.drop_op as usize, 0, 0, 0, 0),
                )
                .expect("couldn't forward drop message");
                break;
            }
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("modal forwarding server exiting");
    xous::destroy_server(xous::SID::from_array(forwarding_config.public_sid))
        .expect("can't destroy my server on exit!");
}
