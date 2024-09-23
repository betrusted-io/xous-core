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
/// 6. (implicit) the memory_message previously held in the `dr` record is dropped, trigging the caller to
///    unblock
/// 7. once you are sure you're finished, call `token_lock = next_lock(&mut work_queue);` to pull any waiting
///    work from the work queue
///
/// Between 5 & 7 is where the TextEntry is weird: because you can "fail" on the return,
/// it doesn't automatically do step 7. It's an extra step that the library implementation
/// does after it does the text validation on its side, once it validates the caller sends
/// a `TextResponseValid` message which pumps the work queue.
mod api;
use api::*;
#[cfg(feature = "ditherpunk")]
use gam::Bitmap;
use gam::modal::*;
use locales::t;
#[cfg(feature = "tts")]
use tts_frontend::TtsFrontend;
use xous::{Message, msg_blocking_scalar_unpack, msg_scalar_unpack, send_message};
use xous_ipc::Buffer;
#[cfg(feature = "tts")]
const TICK_INTERVAL: u64 = 2500;

use std::collections::HashMap;

use bit_field::BitField;
use num_traits::*;

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
    RunBip39(ManagedBip39),
    RunBip39Input(ManagedBip39),
    RunDynamicNotification(DynamicNotification),
    #[cfg(feature = "ditherpunk")]
    RunImage(ManagedImage),
}

const DEFAULT_STYLE: GlyphStyle = gam::SYSTEM_STYLE;

fn main() -> ! {
    #[cfg(not(feature = "ditherpunk"))]
    wrapped_main();

    #[cfg(feature = "ditherpunk")]
    let stack_size = 1024 * 1024;
    #[cfg(feature = "ditherpunk")]
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}
fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let modals_sid = xns.register_name(api::SERVER_NAME_MODALS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", modals_sid);

    #[cfg(feature = "tts")]
    let tt = ticktimer_server::Ticktimer::new().unwrap();

    // we are our own renderer now that we implement deferred responses
    let renderer_cid = xous::connect(modals_sid).expect("couldn't connect to the modal UX renderer");

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
        Opcode::SliderReturn.to_u32().unwrap(),
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
        DEFAULT_STYLE,
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

    let mut token_lock: Option<[u32; 4]> = None;
    #[cfg(feature = "cramium-soc")]
    let trng = cram_hal_service::trng::Trng::new(&xns).unwrap();
    #[cfg(not(feature = "cramium-soc"))]
    let trng = trng::Trng::new(&xns).unwrap();
    // this is a random number that serves as a "default" that cannot be guessed
    let default_nonce =
        [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap()];
    let mut work_queue = Vec::<(xous::MessageSender, [u32; 4])>::new();

    let mut dynamic_notification_listener: Option<xous::MessageSender> = None;
    let mut dynamic_notification_active: bool = false;

    loop {
        let mut msg = xous::receive_message(modals_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
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
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let spec = buffer.to_original::<ManagedPromptWithFixedResponse, _>().unwrap();
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
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let spec = buffer.to_original::<ManagedPromptWithFixedResponse, _>().unwrap();
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
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let spec = buffer.to_original::<ManagedPromptWithTextResponse, _>().unwrap();
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
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
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
            Some(Opcode::Bip39) => {
                let spec = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<ManagedBip39, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunBip39(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::Bip39Input) => {
                let spec = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<ManagedBip39, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunBip39Input(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            #[cfg(feature = "ditherpunk")]
            Some(Opcode::Image) => {
                let spec = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    buffer.to_original::<ManagedImage, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunImage(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::StartProgress) => {
                let spec = {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
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
            Some(Opcode::Slider) => {
                let spec = {
                    let buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    buffer.to_original::<ManagedProgress, _>().unwrap()
                };
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunProgress(spec);
                dr = Some(msg);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::InitiateOp.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't initiate UX op");
            }
            Some(Opcode::StopProgress) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                let token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                send_message(
                    renderer_cid,
                    Message::new_scalar(
                        Opcode::FinishProgress.to_usize().unwrap(),
                        msg.sender.to_usize(),
                        0,
                        0,
                        0,
                    ),
                )
                .expect("couldn't update progress bar");
            }),
            Some(Opcode::AddModalItem) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let manageditem = buffer.to_original::<ManagedListItem, _>().unwrap();
                if manageditem.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!(
                        "Attempt to access modals without a mutex lock. Ignoring. got: {:x?} have: {:x?}",
                        manageditem.token,
                        token_lock
                    );
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
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
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
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let spec = buffer.to_original::<DynamicNotification, _>().unwrap();
                if spec.token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    continue;
                }
                op = RendererState::RunDynamicNotification(spec);
                send_message(
                    renderer_cid,
                    Message::new_scalar(Opcode::DoUpdateDynamicNotification.to_usize().unwrap(), 0, 0, 0, 0),
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
                    Message::new_scalar(Opcode::DoCloseDynamicNotification.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't close dynamic notification");
            }),
            // this got promoted to an external API during the deferred response refactor to eliminate an
            // intermediate state
            Some(Opcode::DoUpdateProgress) => msg_scalar_unpack!(msg, current, _, _, _, {
                let new_percentage = compute_checked_percentage(current as u32, start_work, end_work);
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
                            tts.tts_blocking(t!("progress.increment", locales::LANG)).unwrap();
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
            Some(Opcode::ListenToDynamicNotification) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                let incoming_token = [t0 as u32, t1 as u32, t2 as u32, t3 as u32];
                if incoming_token != token_lock.unwrap_or(default_nonce) {
                    log::warn!("Attempt to access modals without a mutex lock. Ignoring.");
                    xous::return_scalar2(msg.sender, 2, 0).unwrap();
                }
                dynamic_notification_listener = Some(msg.sender); // this defers the response, blocking the caller, while we can proceed onwards.
            }),

            // ------------------ INTERNAL APIS --------------------
            Some(Opcode::InitiateOp) => {
                log::debug!("InitiateOp called");
                match &op {
                    RendererState::RunText(config) => {
                        log::debug!("initiating text entry modal");
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.prompt.as_str()).unwrap();
                        log::info!("setting growable to: {:?}", config.growable);
                        renderer_modal.set_growable(config.growable);
                        renderer_modal.modify(
                            Some(ActionType::TextEntry({
                                let mut ta = text_action.clone();
                                ta.reset_action_payloads(config.fields, config.placeholders.clone());

                                ta
                            })),
                            Some(config.prompt.as_str()),
                            false,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
                        );
                        renderer_modal.activate();
                        log::debug!("should be active!");
                    }
                    RendererState::RunNotification(config) => {
                        let mut notification = gam::modal::Notification::new(
                            renderer_cid,
                            Opcode::NotificationReturn.to_u32().unwrap(),
                        );
                        let text = config.message.as_str();
                        let tmp: String;
                        let qrtext = match &config.qrtext {
                            Some(text) => {
                                tmp = text.to_string();
                                Some(tmp.as_str())
                            }
                            None => None,
                        };
                        notification.set_qrcode(qrtext);
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.message.as_str()).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Notification(notification)),
                            Some(text),
                            false,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunBip39(config) => {
                        let notification = gam::modal::Notification::new(
                            renderer_cid,
                            Opcode::NotificationReturn.to_u32().unwrap(),
                        );
                        let mut text = String::new();
                        if let Some(c) = &config.caption {
                            text.push_str(c.as_str());
                            text.push_str("\n\n");
                        }

                        let phrase = renderer_modal
                            .gam
                            .bytes_to_bip39(&config.bip39_data[..config.bip39_len as usize].to_vec())
                            .unwrap_or(vec![t!("bip39.invalid_bytes", locales::LANG).to_string()]);
                        #[cfg(feature = "hazardous-debug")]
                        log::info!("BIP-39 phrase: {:?}", phrase);

                        for word in phrase {
                            text.push_str(&word);
                            text.push_str(" ");
                        }

                        #[cfg(feature = "tts")]
                        tts.tts_simple(&text).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Notification(notification)),
                            Some(&text),
                            false,
                            None,
                            true,
                            Some(GlyphStyle::Bold),
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunBip39Input(config) => {
                        let b39input = gam::modal::Bip39Entry::new(
                            false,
                            renderer_cid,
                            Opcode::Bip39Return.to_u32().unwrap(),
                        );
                        let mut text = String::new();
                        if let Some(c) = &config.caption {
                            text.push_str(c.as_str());
                        }

                        #[cfg(feature = "tts")]
                        tts.tts_simple(&text).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Bip39Entry(b39input)),
                            Some(&text),
                            false,
                            None,
                            true,
                            Some(GlyphStyle::Bold),
                        );
                        renderer_modal.activate();
                    }
                    #[cfg(feature = "ditherpunk")]
                    RendererState::RunImage(config) => {
                        let mut image =
                            gam::modal::Image::new(renderer_cid, Opcode::ImageReturn.to_u32().unwrap());
                        image.set_bitmap(Some(Bitmap::from(config.tiles)));
                        log::debug!("image: {:x?}", image);
                        renderer_modal.modify(
                            Some(ActionType::Image(image)),
                            None,
                            true,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
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
                        progress_action.set_is_progressbar(!config.user_interaction);
                        progress_action.step = config.step;
                        #[cfg(feature = "tts")]
                        tts.tts_simple(config.title.as_str()).unwrap();
                        renderer_modal.modify(
                            Some(ActionType::Slider(progress_action)),
                            Some(config.title.as_str()),
                            false,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
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
                            radiobuttons.add_item(item.clone());
                            list_hash.insert(item.as_str().to_string(), list_hash.len());
                        }
                        fixed_items.clear();
                        #[cfg(feature = "tts")]
                        {
                            tts.tts_blocking(t!("modals.radiobutton", locales::LANG)).unwrap();
                            tts.tts_blocking(config.prompt.as_str()).unwrap();
                        }
                        renderer_modal.modify(
                            Some(ActionType::RadioButtons(radiobuttons)),
                            Some(config.prompt.as_str()),
                            false,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
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
                            checkbox.add_item(item.clone());
                            list_hash.insert(item.as_str().to_string(), list_hash.len());
                        }
                        fixed_items.clear();
                        #[cfg(feature = "tts")]
                        {
                            tts.tts_blocking(t!("modals.checkbox", locales::LANG)).unwrap();
                            tts.tts_blocking(config.prompt.as_str()).unwrap();
                        }
                        renderer_modal.modify(
                            Some(ActionType::CheckBoxes(checkbox)),
                            Some(config.prompt.as_str()),
                            false,
                            None,
                            true,
                            Some(DEFAULT_STYLE),
                        );
                        renderer_modal.activate();
                    }
                    RendererState::RunDynamicNotification(config) => {
                        if dynamic_notification_active {
                            log::error!(
                                "Dynamic notification already active! Double-calls lead to unpredictable results"
                            );
                        }
                        dynamic_notification_active = true;
                        let mut top_text = String::new();
                        if let Some(title) = &config.title {
                            #[cfg(feature = "tts")]
                            tts.tts_simple(title.as_str()).unwrap();
                            top_text.push_str(title.as_str());
                        }
                        let mut bot_text = String::new();
                        if let Some(text) = &config.text {
                            #[cfg(feature = "tts")]
                            tts.tts_simple(text.as_str()).unwrap();
                            bot_text.push_str(text.as_str());
                        }
                        let mut gutter = gam::modal::Notification::new(
                            renderer_cid,
                            Opcode::HandleDynamicNotificationKeyhit.to_u32().unwrap(),
                        );
                        gutter.set_manual_dismiss(false);
                        // renderer_modal.gam.set_debug_level(log::LevelFilter::Debug);
                        renderer_modal.modify(
                            Some(ActionType::Notification(gutter)),
                            Some(&top_text),
                            config.title.is_none(),
                            Some(&bot_text),
                            config.text.is_none(),
                            Some(DEFAULT_STYLE),
                        );
                        renderer_modal.activate();
                        xous::yield_slice();
                    }
                    RendererState::None => {
                        log::error!("Operation initiated with no argument specified. Ignoring request.");
                        continue;
                    }
                }
            }
            Some(Opcode::FinishProgress) => msg_scalar_unpack!(msg, caller, _, _, _, {
                renderer_modal.gam.relinquish_focus().unwrap();
                op = RendererState::None;
                // unblock the caller, which was forwarded on as the first argument
                xous::return_scalar(xous::sender::Sender::from_usize(caller), 0).ok();
                token_lock = next_lock(&mut work_queue);
                /*
                if work_queue.len() > 0 {
                    let (unblock_pid, token) = work_queue.remove(0);
                    token_lock = Some(token);
                    xous::return_scalar(unblock_pid, 1).unwrap();
                } else {
                    token_lock = None;
                }*/
            }),
            Some(Opcode::DoUpdateDynamicNotification) => match &op {
                RendererState::RunDynamicNotification(config) => {
                    //log::set_max_level(log::LevelFilter::Trace);
                    //renderer_modal.gam.set_debug_level(log::LevelFilter::Debug);
                    let mut top_text = String::new();
                    if let Some(title) = &config.title {
                        top_text.push_str(title.as_str());
                    }
                    let mut bot_text = String::new();
                    if let Some(text) = &config.text {
                        bot_text.push_str(text.as_str());
                    }
                    renderer_modal.modify(
                        None,
                        Some(&top_text),
                        config.title.is_none(),
                        Some(&bot_text),
                        config.text.is_none(),
                        None,
                    );
                    log::debug!("UPDATE_DYN gid: {:?}", renderer_modal.canvas);
                    renderer_modal.redraw();
                    xous::yield_slice();
                    //log::set_max_level(log::LevelFilter::Info);
                    //renderer_modal.gam.set_debug_level(log::LevelFilter::Info);
                }
                _ => {
                    log::error!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                    panic!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                }
            },
            Some(Opcode::DoCloseDynamicNotification) => {
                renderer_modal.gam.relinquish_focus().unwrap();
                dynamic_notification_active = false;
                op = RendererState::None;
                if let Some(sender) = dynamic_notification_listener.take() {
                    // unblock the listener with no key hit response
                    xous::return_scalar2(sender, 0, 0).unwrap();
                }
                token_lock = next_lock(&mut work_queue);
            }
            Some(Opcode::HandleDynamicNotificationKeyhit) => msg_scalar_unpack!(msg, k, _, _, _, {
                log::debug!("Dynamic kbd hit: {}({})", k, char::from_u32(k as u32).unwrap_or(' '));
                if let Some(sender) = dynamic_notification_listener.take() {
                    xous::return_scalar2(sender, 1, k).unwrap();
                }
            }),
            Some(Opcode::SliderReturn) => match op {
                RendererState::RunProgress(_) => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let item = buffer.to_original::<SliderPayload, _>().unwrap();

                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(origin.body.memory_message_mut().unwrap())
                        };

                        response.replace(item).unwrap();
                        op = RendererState::None;

                        token_lock = next_lock(&mut work_queue);
                    } else {
                        log::error!("Ux routine returned but no origin was recorded");
                        panic!("Ux routine returned but no origin was recorded");
                    }
                }
                _ => {
                    log::warn!("got weird stuff on slider return, ignoring");
                }
            },
            Some(Opcode::TextEntryReturn) => match op {
                RendererState::RunText(_config) => {
                    renderer_modal.set_growable(false); // reset the growable state, it's assumed to be default false
                    log::trace!("validating text entry modal");
                    let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let text = buf.to_original::<gam::modal::TextEntryPayloads, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(origin.body.memory_message_mut().unwrap())
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
                    log::error!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                    panic!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
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
                    RendererState::RunNotification(_) | RendererState::RunBip39(_) => {
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
                        panic!(
                            "UX return opcode does not match our current operation in flight. This is a serious internal error."
                        );
                    }
                }
            }
            Some(Opcode::Bip39Return) => match op {
                RendererState::RunBip39Input(_config) => {
                    let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let b39 = buf.to_original::<gam::modal::Bip39EntryPayload, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(origin.body.memory_message_mut().unwrap())
                        };
                        let mut spec = response.to_original::<ManagedBip39, _>().unwrap();
                        spec.bip39_data[..b39.len as usize].copy_from_slice(&b39.data[..b39.len as usize]);
                        spec.bip39_len = b39.len;

                        response.replace(spec).unwrap();
                        op = RendererState::None;
                        token_lock = next_lock(&mut work_queue);
                    } else {
                        log::error!("Ux routine returned but no origin was recorded");
                        panic!("Ux routine returned but no origin was recorded");
                    }
                }
                RendererState::None => {
                    log::warn!("Text entry detected a fat finger event, ignoring.")
                }
                _ => {
                    log::error!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                    panic!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                }
            },
            #[cfg(feature = "ditherpunk")]
            Some(Opcode::ImageReturn) => {
                match op {
                    RendererState::RunImage(_) => {
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
                        panic!(
                            "UX return opcode does not match our current operation in flight. This is a serious internal error."
                        );
                    }
                }
            }
            Some(Opcode::Gutter) => {
                log::info!("gutter op, doing nothing");
            }
            Some(Opcode::RadioReturn) => match op {
                RendererState::RunRadio(_config) => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let item = buffer.to_original::<RadioButtonPayload, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(origin.body.memory_message_mut().unwrap())
                        };
                        response.replace(item.clone()).unwrap();
                        op = RendererState::None;
                        match list_hash.get(item.as_str()) {
                            Some(index) => {
                                match index {
                                    0..=31 => drop(list_selected.set_bit(*index, true)),
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
                    log::error!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                    panic!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                }
            },
            Some(Opcode::CheckBoxReturn) => match op {
                RendererState::RunCheckBox(_config) => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let item = buffer.to_original::<CheckBoxPayload, _>().unwrap();
                    if let Some(mut origin) = dr.take() {
                        let mut response = unsafe {
                            Buffer::from_memory_message_mut(origin.body.memory_message_mut().unwrap())
                        };
                        response.replace(item.clone()).unwrap();
                        op = RendererState::None;
                        for (_, check_item) in item.payload().iter().enumerate() {
                            match check_item {
                                Some(item) => match list_hash.get(item.as_str()) {
                                    Some(index) => {
                                        match index {
                                            0..=31 => drop(list_selected.set_bit(*index, true)),
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
                    log::error!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
                    panic!(
                        "UX return opcode does not match our current operation in flight. This is a serious internal error."
                    );
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
