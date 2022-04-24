#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/// A modals initiator will:
///
/// 1. unpack the specification from the incoming message
/// 2. check the API token
/// 3. if valid, set the `op` with the appropriate enum to encapsulate the spec
/// 4. store the memory message in the `dr` (deferred response) Option
/// 5. send a message to itself to initiate the operation
///
/// The operation proceeds within this server, perhaps through multiple states.
///
/// On completion, the final state(s) do the following:
///
/// 1. unpack the return data message, which came from an internal (within-modals) state
/// 2. `take()` the `dr` record, preparing for its lifetime to be ended
/// 3. unpack the `memory_message` inside the `dr` record into a Buffer
/// 4. `replace()` the return data into the `Buffer`
/// 5. Set the op to `RenderState::None`
/// 6. (implicit) the memory_message previously held in the `dr` record is dropped, trigging the caller to unblock
/// 7. once you are sure you're finished, call `token_lock = next_lock(&mut work_queue);` to pull any waiting work from the work queue
///
/// Between 5 & 7 is where the TextEntry is weird: because you can "fail" on the return,
/// it doesn't automatically do step 7. It's an extra step that the library implementation
/// does after it does the text validation on its side, once it validates the caller sends
/// a `TextResponseValid` message which pumps the work queue.
mod api;
use api::*;
mod tests;

use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;

use gam::modal::*;
#[cfg(feature = "tts")]
use locales::t;
#[cfg(feature = "tts")]
use tts_frontend::TtsFrontend;
#[cfg(feature = "tts")]
const TICK_INTERVAL: u64 = 2500;

use bit_field::BitField;
use num_traits::*;
use std::collections::HashMap;

#[derive(Debug)]
enum RendererState {
    /// idle state
    None,
    /// running state
    RunRadio(ManagedPromptWithFixedResponse),
    RunCheckBox(ManagedPromptWithFixedResponse),
    RunText(ManagedPromptWithTextResponse),
    RunProgress(ManagedProgress),
    RunNotification(ManagedNotification),
    RunDynamicNotification(DynamicNotification),
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let modals_sid = xns
        .register_name(api::SERVER_NAME_MODALS, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", modals_sid);

    let tt = ticktimer_server::Ticktimer::new().unwrap();

    // we are our own renderer now that we implement deferred responses
    let renderer_cid =
        xous::connect(modals_sid).expect("couldn't connect to the modal UX renderer");

    #[cfg(feature = "tts")]
    let tts = TtsFrontend::new(&xns).unwrap();
    #[cfg(feature = "tts")]
    let mut last_tick = tt.elapsed_ms();

    // current opcode being processed by the modals server
    let mut op = RendererState::None;
    // the message for the deferred response
    let mut dr: Option<xous::MessageEnvelope> = None;

    // build the core data structure here
    let mut text_action: TextEntry = Default::default();
    text_action.action_conn = renderer_cid;
    text_action.action_opcode = Opcode::TextEntryReturn.to_u32().unwrap();

    let mut fixed_items = Vec::<ItemName>::new();
    let mut progress_action = Slider::new(
        renderer_cid,
        Opcode::Gutter.to_u32().unwrap(),
        0,
        100,
        1,
        Some("%"),
        0,
        true,
        true,
    );
    let mut last_percentage = 0;
    let mut start_work: u32 = 0;
    let mut end_work: u32 = 100;
    let mut renderer_modal = Modal::new(
        gam::SHARED_MODAL_NAME,
        ActionType::TextEntry(text_action.clone()),
        Some("Placeholder"),
        None,
        GlyphStyle::Regular,
        8,
    );
    renderer_modal.spawn_helper(
        modals_sid,
        renderer_modal.sid,
        Opcode::ModalRedraw.to_u32().unwrap(),
        Opcode::ModalKeypress.to_u32().unwrap(),
        Opcode::ModalDrop.to_u32().unwrap(),
    );

    let mut list_hash = HashMap::<String, usize>::new();
    let mut list_selected = 0u32;

    if cfg!(feature = "ux_tests") {
        tt.sleep_ms(1000).unwrap();
        tests::spawn_test();
    }

    let mut token_lock: Option<[u32; 4]> = None;
    let trng = trng::Trng::new(&xns).unwrap();
    // this is a random number that serves as a "default" that cannot be guessed
    let default_nonce = [
        trng.get_u32().unwrap(),
        trng.get_u32().unwrap(),
        trng.get_u32().unwrap(),
        trng.get_u32().unwrap(),
    ];
    let mut work_queue = Vec::<(xous::MessageSender, [u32; 4])>::new();

    loop {
        let mut msg = xous::receive_message(modals_sid).unwrap();
        log::debug!("message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            // ------------------ EXTERNAL APIS --------------------
            Some(Opcode::GetMutex) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                let incoming_token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token_lock.is_none() {
                    token_lock = Some(incoming_token);
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    work_queue.push((msg.sender, incoming_token));
                }
            }),
            Some(Opcode::PromptWithFixedResponse) => {
                let spec = {
                    let mut buffer = unsafe {
                        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                    };
                    let spec = buffer
                        .to_original::<ManagedPromptWithFixedResponse, _>()
                        .unwrap();
                    if spec.token != token_lock.unwrap_or(default_nonce) {
                        log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                        buffer.replace(ItemName::new("internal error")).unwrap();
                        continue;
                    }
                    spec
                };
                op = RendererState::RunRadio(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::PromptWithMultiResponse) => {
                let spec = {
                    let mut buffer = unsafe {
                        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                    };
                    let spec = buffer
                        .to_original::<ManagedPromptWithFixedResponse, _>()
                        .unwrap();
                    if spec.token != token_lock.unwrap_or(default_nonce) {
                        log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                        buffer.replace(CheckBoxPayload::new()).unwrap();
                        continue;
                    }
                    spec
                };
                op = RendererState::RunCheckBox(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::PromptWithTextResponse) => {
                let spec = {
                    let mut buffer = unsafe {
                        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                    };
                    let spec = buffer
                        .to_original::<ManagedPromptWithTextResponse, _>()
                        .unwrap();
                    if spec.token != token_lock.unwrap_or(default_nonce) {
                        log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                        buffer.replace(TextEntryPayload::new()).unwrap();
                        continue;
                    }
                    spec
                };
                op = RendererState::RunText(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::Notification) => {
                let spec = {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<ManagedNotification, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunNotification(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::StartProgress) => {
                let spec = {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<ManagedProgress, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunProgress(spec);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::StopProgress) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::FinishProgress.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't update progress bar");
            }),
            Some(Opcode::AddModalItem) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let manageditem = buffer.to_original::<ManagedListItem, _>().unwrap();
                if manageditem.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring. got: {:x?} have: {:x?}", manageditem.token, token_lock);
                    continue;
                }
                fixed_items.push(manageditem.item);
            }
            Some(Opcode::GetModalIndex) => {
                xous::return_scalar(msg.sender, list_selected as usize)
                    .expect("couldn't return list selected");
            }
            Some(Opcode::DynamicNotification) => {
                let spec = {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<DynamicNotification, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunDynamicNotification(spec);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::UpdateDynamicNotification) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<DynamicNotification, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunDynamicNotification(spec);
                send_message(
                    renderer_cid,
                    Message::new_scalar(
                        Opcode::DoUpdateDynamicNotification.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::CloseDynamicNotification) => msg_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                send_message(
                    renderer_cid,
                    Message::new_scalar(
                        Opcode::DoCloseDynamicNotification.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .expect("couldn't close dynamic notification");
            }),
            // this got promoted to an external API during the deferred response refactor to eliminate an intermediate state
            Some(Opcode::DoUpdateProgress) => msg_scalar_unpack!(msg, current, _, _, _, {
                let new_percentage =
                    compute_checked_percentage(current as u32, start_work, end_work);
                log::trace!(
                    "percentage: {}, current: {}, start: {}, end: {}",
                    new_percentage,
                    current,
                    start_work,
                    end_work
                );
                if new_percentage != last_percentage {
                    last_percentage = new_percentage;
                    progress_action.set_state(last_percentage);
                    #[cfg(feature = "tts")]
                    {
                        if tt.elapsed_ms() - last_tick > TICK_INTERVAL {
                            tts.tts_blocking(t!("progress.increment", xous::LANG))
                                .unwrap();
                            last_tick = tt.elapsed_ms();
                        }
                    }
                    renderer_modal.modify(
                        Some(ActionType::Slider(progress_action)),
                        None,
                        false,
                        None,
                        false,
                        None,
                    );
                    renderer_modal.redraw();
                    xous::yield_slice(); // give time for the GAM to redraw
                }
            }),

            // ------------------ INTERNAL APIS --------------------
            Some(Opcode::InitiateOp) => {
                log::debug!("InitiateOp called");
                match op {
                    RendererState::RunText(config) => {
                        log::debug!("initiating text entry modal");
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.prompt.as_str().unwrap()).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::TextEntry({
                                let mut ta = text_action.clone();
                                ta.reset_action_payloads(config.fields, config.placeholders);

                                ta
                            })),
                            Some(config.prompt.as_str().unwrap()),
                            false,
                            None,
                            true,
                            None,
                        );
                        renderer_modal.activate();
                        log::debug!("should be active!");
                    }
                    RendererState::RunNotification(config) => {
                        let mut notification = gam::modal::Notification::new(
                            renderer_cid,
                            Opcode::NotificationReturn.to_u32().unwrap(),
                        );
                        let text = config.message.as_str().unwrap();
                        let tmp: String;
                        let qrtext = match config.qrtext {
                            Some(text) => {
                                tmp = text.to_string();
                                Some(tmp.as_str())
                            }
                            None => None,
                        };
                        notification.set_qrcode(qrtext);
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.message.as_str().unwrap()).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Notification(notification)),
                            Some(text),
                            false,
                            None,
                            true,
                            None,
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunProgress(config) => {
                        start_work = config.start_work;
                        end_work = config.end_work;
                        last_percentage =
                            compute_checked_percentage(config.current_work, start_work, end_work);
                        log::debug!(
                            "init percentage: {}, current: {}, start: {}, end: {}",
                            last_percentage,
                            config.current_work,
                            start_work,
                            end_work
                        );
                        progress_action.set_state(last_percentage);
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.title.as_str().unwrap()).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Slider(progress_action)),
                            Some(config.title.as_str().unwrap()),
                            false,
                            None,
                            true,
                            None,
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunRadio(config) => {
                        let mut radiobuttons = gam::modal::RadioButtons::new(
                            renderer_cid,
                            Opcode::RadioReturn.to_u32().unwrap(),
                        );
                        list_hash.clear();
                        list_selected = 0u32;
                        for item in fixed_items.iter() {
                            radiobuttons.add_item(*item);
                            list_hash.insert(item.as_str().to_string(), list_hash.len());
                        }
                        fixed_items.clear();
                        #[cfg(feature = "tts")]
                        {
                            tts.tts_blocking(t!("modals.radiobutton", xous::LANG))
                                .unwrap();
                            tts.tts_blocking(config.prompt.as_str().unwrap()).unwrap();
                        }
                        renderer_modal.modify(
                            Some(ActionType::RadioButtons(radiobuttons)),
                            Some(config.prompt.as_str().unwrap()),
                            false,
                            None,
                            true,
                            None,
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunCheckBox(config) => {
                        let mut checkbox = gam::modal::CheckBoxes::new(
                            renderer_cid,
                            Opcode::CheckBoxReturn.to_u32().unwrap(),
                        );
                        list_hash.clear();
                        list_selected = 0u32;
                        for item in fixed_items.iter() {
                            checkbox.add_item(*item);
                            list_hash.insert(item.as_str().to_string(), list_hash.len());
                        }
                        fixed_items.clear();
                        #[cfg(feature = "tts")]
                        {
                            tts.tts_blocking(t!("modals.checkbox", xous::LANG)).unwrap();
                            tts.tts_blocking(config.prompt.as_str().unwrap()).unwrap();
                        }
                        renderer_modal.modify(
                            Some(ActionType::CheckBoxes(checkbox)),
                            Some(config.prompt.as_str().unwrap()),
                            false,
                            None,
                            true,
                            None,
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunDynamicNotification(config) => {
                        let mut top_text = String::new();
                        if let Some(title) = config.title {
                            #[cfg(feature = "tts")]
                            tts.tts_simple(title.as_str().unwrap()).unwrap();
                            top_text.push_str(title.as_str().unwrap());
                        }
                        let mut bot_text = String::new();
                        if let Some(text) = config.text {
                            #[cfg(feature = "tts")]
                            tts.tts_simple(text.as_str().unwrap()).unwrap();
                            bot_text.push_str(text.as_str().unwrap());
                        }
                        let mut gutter = gam::modal::Notification::new(
                            renderer_cid,
                            Opcode::Gutter.to_u32().unwrap(),
                        );
                        gutter.set_manual_dismiss(false);
                        renderer_modal.modify(
                            Some(ActionType::Notification(gutter)),
                            Some(&top_text),
                            config.title.is_none(),
                            Some(&bot_text),
                            config.text.is_none(),
                            None,
                        );
                        renderer_modal.activate();
                    }
                    RendererState::None => {
                        log::error!(
                            "Operation initiated with no argument specified. Ignoring request."
                        );
                        continue;
                    }
                }
            }
            Some(Opcode::FinishProgress) => {
                renderer_modal.gam.relinquish_focus().unwrap();
                op = RendererState::None;
                token_lock = next_lock(&mut work_queue);
                /*
                if work_queue.len() > 0 {
                    let (unblock_pid, token) = work_queue.remove(0);
                    token_lock = Some(token);
                    xous::return_scalar(unblock_pid, 1).unwrap();
                } else {
                    token_lock = None;
                }*/
            }
            Some(Opcode::DoUpdateDynamicNotification) => match op {
                RendererState::RunDynamicNotification(config) => {
                    let mut top_text = String::new();
                    if let Some(title) = config.title {
                        top_text.push_str(title.as_str().unwrap());
                    }
                    let mut bot_text = String::new();
                    if let Some(text) = config.text {
                        bot_text.push_str(text.as_str().unwrap());
                    }
                    renderer_modal.modify(
                        None,
                        Some(&top_text),
                        config.title.is_none(),
                        Some(&bot_text),
                        config.text.is_none(),
                        None,
                    );
                    renderer_modal.redraw();
                }
                _ => {
                    log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                    panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                }
            },
            Some(Opcode::DoCloseDynamicNotification) => {
                renderer_modal.gam.relinquish_focus().unwrap();
                op = RendererState::None;
                token_lock = next_lock(&mut work_queue);
            }
            Some(Opcode::TextEntryReturn) => match op {
                RendererState::RunText(_config) => {
                    log::trace!("validating text entry modal");
                    let buf =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let text = buf
                        .to_original::<gam::modal::TextEntryPayloads, _>()
                        .unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(
                                origin.body.memory_message_mut().unwrap(),
                            )
                        };
                        response.replace(text).unwrap();
                        op = RendererState::None;
                    } else {
                        log::error!("Ux routine returned but no origin was recorded");
                        panic!("Ux routine returned but no origin was recorded");
                    }
                }
                RendererState::None => {
                    log::warn!("Text entry detected a fat finger event, ignoring.")
                }
                _ => {
                    log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                    panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                }
            },
            Some(Opcode::TextResponseValid) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                let incoming_token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if incoming_token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                } else {
                    token_lock = next_lock(&mut work_queue);
                }
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::NotificationReturn) => {
                match op {
                    RendererState::RunNotification(_) => {
                        op = RendererState::None;
                        dr.take(); // unblocks the caller, but without any response data
                        token_lock = next_lock(&mut work_queue);
                    }
                    RendererState::None => {
                        log::warn!("Notification detected a fat finger event, ignoring.")
                    }
                    _ => {
                        log::error!(
                            "UX return opcode does not match our current operation in flight: {:?}",
                            op
                        );
                        panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                    }
                }
            }
            Some(Opcode::Gutter) => {
                log::info!("gutter op, doing nothing");
            }
            Some(Opcode::RadioReturn) => match op {
                RendererState::RunRadio(_config) => {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let item = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(
                                origin.body.memory_message_mut().unwrap(),
                            )
                        };
                        response.replace(item).unwrap();
                        op = RendererState::None;
                        match list_hash.get(item.as_str()) {
                            Some(index) => {
                                match index {
                                    0...31 => drop(list_selected.set_bit(*index, true)),
                                    _ => log::warn!("invalid bitfield index"),
                                };
                            }
                            None => log::warn!("failed to set list_selected index"),
                        }
                    } else {
                        log::error!("Ux routine returned but no origin was recorded");
                        panic!("Ux routine returned but no origin was recorded");
                    }
                    token_lock = next_lock(&mut work_queue);
                }
                RendererState::None => {
                    log::warn!("Radio buttons detected a fat finger event, ignoring.")
                }
                _ => {
                    log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                    panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                }
            },
            Some(Opcode::CheckBoxReturn) => match op {
                RendererState::RunCheckBox(_config) => {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let item = buffer.to_original::<CheckBoxPayload, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(
                                origin.body.memory_message_mut().unwrap(),
                            )
                        };
                        response.replace(item).unwrap();
                        op = RendererState::None;
                        for (_, check_item) in item.payload().iter().enumerate() {
                            match check_item {
                                Some(item) => match list_hash.get(item.as_str()) {
                                    Some(index) => {
                                        match index {
                                            0...31 => drop(list_selected.set_bit(*index, true)),
                                            _ => log::warn!("invalid bitfield index"),
                                        };
                                    }
                                    None => log::warn!("failed to set list_selected index"),
                                },
                                None => {}
                            }
                        }
                    } else {
                        log::error!("Ux routine returned but no origin was recorded");
                        panic!("Ux routine returned but no origin was recorded");
                    }
                    token_lock = next_lock(&mut work_queue);
                }
                RendererState::None => {
                    log::warn!("Check boxes detected a fat finger event, ignoring.")
                }
                _ => {
                    log::error!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                    panic!("UX return opcode does not match our current operation in flight. This is a serious internal error.");
                }
            },
            Some(Opcode::ModalRedraw) => {
                renderer_modal.redraw();
            }
            Some(Opcode::ModalKeypress) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                renderer_modal.key_event(keys);
            }),
            Some(Opcode::ModalDrop) => {
                // this guy should never quit, it's a core OS service
                panic!("Password modal for PDDB quit unexpectedly");
            }

            Some(Opcode::Quit) => {
                log::warn!("Shared modal UX handler exiting.");
                break;
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(modals_sid).unwrap();
    xous::destroy_server(modals_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn compute_checked_percentage(current: u32, start: u32, end: u32) -> u32 {
    if end <= start {
        100
    } else {
        if current < start {
            0
        } else if current >= end {
            100
        } else {
            // do math in higher precision because we could overflow a u32
            (((current as u64 - start as u64) * 100) / (end as u64 - start as u64)) as u32
        }
    }
}

fn next_lock(work_queue: &mut Vec<(xous::MessageSender, [u32; 4])>) -> Option<[u32; 4]> {
    if work_queue.len() > 0 {
        /*
        log::debug!("pending:");
        for (pid, tok) in work_queue.iter() {
            log::debug!("pid: {:x?}, tok: {:x?}", pid, tok);
        }
        */
        let (unblock_pid, token) = work_queue.remove(0);
        log::debug!("next token: {:x?}", token);
        xous::return_scalar(unblock_pid, 1).unwrap();
        Some(token)
    } else {
        None
    }
}
