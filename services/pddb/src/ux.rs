use gam::modal::*;
use locales::t;
use num_traits::*;
use xous::msg_scalar_unpack;
use xous::{CID, SID};
use xous_ipc::Buffer;

use crate::BasisRequestPassword;
/*
Conclusions:

- PDDB will get *just* a password modal handler. That is all
  (see UxInitRequestPassword from RootKeys modal; create a thread that handles
  this local to PDDB, which takes in a blocking scalar to request the password,
  and then spawns yet another thread to handle the actual password request.)
- The generic UX handler will be split into a managed server

What we really want from the UX helper is a compound, blocking call which encapsulates
complex requests (prompt -> list response) and progress bars, with a simple blocking or
non-blocking call.

I think non-blocking calls are pretty straight forward, but the blocking call is harder, because
the structure of the UX handler is like this:

The (bouncer) is a single-use server to filter UX server opcodes from the GAM server.

UX Manager          UX Renderer                     GAM server              Keyboard server
Do UX op        ->  raise             ->            compute redraw area
                    redraw manager  <-(bouncer)<-   redraw trigger
                    (user input)
                    key handler     <-(bouncer)<-   event              <-   keyboard events
                    draw updates      ->            compute redraw area
                    redraw manager  <-(bouncer)<-   redraw trigger
                    (user closes box)
                    lower             ->            compute redraw
Continue      <-    response


UX handler thread -> raise message ->
GAM server
GAM Modal Object -> redraw -> gam requests

*/
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PwManagerOpcode {
    // blocking call to get a password
    RequestPassword,
    PwReturn,
    ModalRedraw,
    ModalKeypress,
    ModalDrop,
    Quit,
}

pub(crate) fn password_ux_manager(
    // the CID of the main loop, as a backchannel for async callbacks.
    _main_cid: CID,
    // the SID we're to use for our loop handler, for getting requests
    ux_sid: SID,
) {
    let ux_cid = xous::connect(ux_sid).unwrap();
    // create a thread that just handles the redrawing requests
    // build the core data structure here
    let mut password_action = TextEntry::new(
        true,
        TextEntryVisibility::LastChars,
        ux_cid,
        PwManagerOpcode::PwReturn.to_u32().unwrap(),
        vec![TextEntryPayload::new()],
        None,
    );
    password_action.reset_action_payloads(1, None);

    let mut pddb_modal = Modal::new(
        gam::PDDB_MODAL_NAME,
        ActionType::TextEntry(password_action.clone()),
        Some(t!("pddb.password", locales::LANG)),
        None,
        gam::SYSTEM_STYLE,
        8,
    );
    pddb_modal.spawn_helper(
        ux_sid,
        pddb_modal.sid,
        PwManagerOpcode::ModalRedraw.to_u32().unwrap(),
        PwManagerOpcode::ModalKeypress.to_u32().unwrap(),
        PwManagerOpcode::ModalDrop.to_u32().unwrap(),
    );

    let mut dr: Option<xous::MessageEnvelope> = None;

    // the main loop merely wraps a blocking wrapper around the renderer thread
    loop {
        let msg = xous::receive_message(ux_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(PwManagerOpcode::RequestPassword) => {
                let db_name = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let request = buffer.to_original::<BasisRequestPassword, _>().unwrap();
                    request.db_name
                };
                pddb_modal.modify(
                    Some(ActionType::TextEntry(password_action.clone())),
                    Some(t!("pddb.password", locales::LANG)),
                    false,
                    Some(
                        format!("{}'{}'", t!("pddb.password_for", locales::LANG), db_name.as_str()).as_str(),
                    ),
                    false,
                    None,
                );
                log::info!("{}PDDB.REQPW,{},{}", xous::BOOKEND_START, db_name.as_str(), xous::BOOKEND_END);
                pddb_modal.activate();
                dr = Some(msg);
            }
            Some(PwManagerOpcode::PwReturn) => {
                let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let pw = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();

                if let Some(mut response) = dr.take() {
                    let mut buffer = unsafe {
                        Buffer::from_memory_message_mut(response.body.memory_message_mut().unwrap())
                    };
                    let mut request = buffer.to_original::<BasisRequestPassword, _>().unwrap();
                    request.plaintext_pw = Some(String::from(pw.first().as_str()));
                    // return the password to the caller
                    buffer.replace(request).unwrap();
                    // response goes out of scope here and calls Drop which returns the message
                } else {
                    log::error!("Password return received, but no deferred response on record");
                }

                pw.first().volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                buf.volatile_clear();
            }
            Some(PwManagerOpcode::ModalRedraw) => {
                pddb_modal.redraw();
            }
            Some(PwManagerOpcode::ModalKeypress) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                pddb_modal.key_event(keys);
            }),
            Some(PwManagerOpcode::ModalDrop) => {
                // this guy should never quit, it's a core OS service
                panic!("Password modal for PDDB quit unexpectedly");
            }
            Some(PwManagerOpcode::Quit) => {
                log::warn!("received quit on PDDB password UX renderer loop");
                xous::return_scalar(msg.sender, 0).unwrap();
                break;
            }

            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    xous::destroy_server(ux_sid).unwrap();
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UxOpcode {
    // add UX opcodes here, separate from the main loop's
    Format,
    OkCancelNotice,
    OkNotice,
    UnlockBasis,
    LockBasis,
    LockAllBasis,
    Scuttle,

    PasswordReturn,
    ModalRedraw,
    ModalKeys,
    ModalDrop,
    Gutter,
    Quit,
}
