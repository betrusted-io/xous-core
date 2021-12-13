use xous::{SID, CID};

use xous::{msg_scalar_unpack, send_message};
use xous_ipc::Buffer;

use std::sync::{Arc, Mutex};
use std::thread;
use core::sync::atomic::{AtomicBool, Ordering};

use num_traits::*;

use gam::modal::*;

use locales::t;
use std::fmt::Write;

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

I thnk non-blocking calls are pretty straight forward, but the blocking call is harder, because
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
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PwRendererOpcode {
    RaisePwModal,
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
    ux_sid: SID) {

    let tt = ticktimer_server::Ticktimer::new().unwrap();

    let renderer_sid = xous::create_server().expect("couldn't create a server for the password UX renderer");
    let renderer_cid = xous::connect(renderer_sid).expect("couldn't connect to the password UX renderer");
    let plaintext_pw = Arc::new(Mutex::new(xous_ipc::String::<{crate::api::PASSWORD_LEN}>::new()));

    let renderer_active = Arc::new(AtomicBool::new(false));
    // create a thread that just handles the redrawing requests
    let redraw_handle = thread::spawn({
        let renderer_active = Arc::clone(&renderer_active);
        let plaintext_pw = Arc::clone(&plaintext_pw);
        move || {
            // build the core data structure here
            let password_action = TextEntry {
                is_password: true,
                visibility: TextEntryVisibility::LastChars,
                action_conn: renderer_cid,
                action_opcode: PwRendererOpcode::PwReturn.to_u32().unwrap(),
                action_payload: TextEntryPayload::new(),
                validator: None,
            };

            let mut pddb_modal =
                Modal::new(
                    crate::api::PDDB_MODAL_NAME,
                    ActionType::TextEntry(password_action),
                    Some(t!("pddb.password", xous::LANG)),
                    None,
                    GlyphStyle::Small,
                    8
                );
            pddb_modal.spawn_helper(renderer_sid, pddb_modal.sid,
                PwRendererOpcode::ModalRedraw.to_u32().unwrap(),
                PwRendererOpcode::ModalKeypress.to_u32().unwrap(),
                PwRendererOpcode::ModalDrop.to_u32().unwrap(),
            );

            loop {
                let msg = xous::receive_message(renderer_sid).unwrap();
                log::debug!("message: {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PwRendererOpcode::RaisePwModal) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let db_name = buffer.to_original::<xous_ipc::String::<{crate::api::BASIS_NAME_LEN}>, _>().unwrap();
                        pddb_modal.modify(
                            Some(ActionType::TextEntry(password_action)),
                            Some(t!("pddb.password", xous::LANG)), false,
                            Some(db_name.as_str().unwrap()), false, None
                        );
                        pddb_modal.activate();
                    }
                    Some(PwRendererOpcode::PwReturn) => {
                        let mut buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let mut pw = buf.to_original::<gam::modal::TextEntryPayload, _>().unwrap();

                        plaintext_pw.lock().unwrap().clear();
                        write!(plaintext_pw.lock().unwrap(), "{}", pw.as_str()).expect("couldn't transfer password to local buffer");

                        pw.volatile_clear(); // ensure the data is destroyed after sending to the keys enclave
                        buf.volatile_clear();

                        // this resumes the waiting Ux Manager thread
                        renderer_active.store(false, Ordering::SeqCst);
                    },
                    Some(PwRendererOpcode::ModalRedraw) => {
                        pddb_modal.redraw();
                    },
                    Some(PwRendererOpcode::ModalKeypress) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                        let keys = [
                            if let Some(a) = core::char::from_u32(k1 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k2 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k3 as u32) {a} else {'\u{0000}'},
                            if let Some(a) = core::char::from_u32(k4 as u32) {a} else {'\u{0000}'},
                        ];
                        pddb_modal.key_event(keys);
                    }),
                    Some(PwRendererOpcode::ModalDrop) => { // this guy should never quit, it's a core OS service
                        panic!("Password modal for PDDB quit unexpectedly");
                    },
                    Some(PwRendererOpcode::Quit) => {
                        log::warn!("received quit on PDDB password UX renderer loop");
                        xous::return_scalar(msg.sender, 0).unwrap();
                        break;
                    },
                    None => {
                        log::error!("Couldn't convert opcode: {:?}", msg);
                    }
                }
            }
            xous::destroy_server(renderer_sid).unwrap();
        }
    });

    // the main loop merely wraps a blocking wrapper around the renderer thread
    loop {
        let mut msg = xous::receive_message(ux_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(PwManagerOpcode::RequestPassword) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut request = buffer.to_original::<BasisRequestPassword, _>().unwrap();
                let fwd_buf = Buffer::into_buf(request.db_name).unwrap();
                renderer_active.store(true, Ordering::SeqCst);
                fwd_buf.lend(renderer_cid, PwRendererOpcode::RaisePwModal.to_u32().unwrap()).expect("couldn't request renderer to raise the password modal");
                // this returns almost immediately, because the Ux Manager thread can't block (it has to manage rendering), but the password hasn't come back yet.
                // so...."busy wait" by polling once every 100ms until the Ux is finished with its task.
                // I can't seem to figure out a way to do this using a solely asynchronous method. I really want a spsc queue, but these don't exist in xous stdlib yet.
                // apparently the thing I'm looking for is a `condvar`, which is coming Real Soon Now.
                while renderer_active.load(Ordering::SeqCst) {
                    tt.sleep_ms(100).unwrap();
                }
                // when this unblocks, the password request cycle is done.
                let retpw = xous_ipc::String::<{crate::api::PASSWORD_LEN}>::from_str(plaintext_pw.lock().unwrap().as_str().unwrap());
                request.plaintext_pw = Some(retpw);

                // return the password to the caller
                buffer.replace(request).unwrap();
            },
            Some(PwManagerOpcode::Quit) => {
                log::warn!("PDDB password handler exiting.");
                send_message(
                    renderer_cid,
                    xous::Message::new_blocking_scalar(PwRendererOpcode::Quit.to_usize().unwrap(),
                    0, 0, 0, 0)).unwrap();
                xous::return_scalar(msg.sender, 0).unwrap();
                break
            },
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    redraw_handle.join().expect("redraw thread didn't quit as expected");
    unsafe{xous::disconnect(renderer_cid).unwrap()}; // can't remember if this is...necessary? it'll throw an error if it's not.
    xous::destroy_server(ux_sid).unwrap();
}
