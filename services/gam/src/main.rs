#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

mod api;
use api::*;
mod status;
use status::*;
mod canvas;
use canvas::*;
mod menu;
use menu::*;

mod tokens;
use tokens::*;
mod layouts;
use layouts::*;

use graphics_server::*;
use ime_plugin_api::{ImeFrontEndApi, ImefDescriptor};

use log::info;

use heapless::FnvIndexMap;

use num_traits::*;
use xous_ipc::{Buffer, String};
use api::Opcode;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};

use core::{sync::atomic::{AtomicU32, Ordering}};

use enum_dispatch::enum_dispatch;

//// todo:
// - create menu server
// - add auth tokens to audio streams, so less trusted processes can make direct connections to the codec and reduce latency

#[derive(PartialEq, Eq)]
pub(crate) enum LayoutBehavior {
    /// a layout that can render over others, takes focus, and only dismissed if explicitly dismissed
    Alert,
    /// a layout that assumes it has the full screen and is the primary content when visible
    App,
}

#[enum_dispatch]
pub(crate) trait LayoutApi {
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<(), xous::Error>;
    // for Chats, this resizes the height of the input area; for menus, it resizes the overall height
    fn resize_height(&mut self, gfx: &graphics_server::Gfx, new_height: i16, status_canvas: &Canvas, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<Point, xous::Error>;
    fn get_input_canvas(&self) -> Option<Gid> { None }
    fn get_prediction_canvas(&self) -> Option<Gid> { None }
    fn get_content_canvas(&self) -> Gid; // layouts always have a content canvas
    // when the argument is true, the context is moved "onscreen" by moving the canvases into the screen clipping rectangle
    // when false, the context is moved "offscreen" by moving the canvases outside the screen clipping rectangle
    // note that this visibility state is an independent variable from the trust level draw-ability
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>);
    fn behavior(&self) -> LayoutBehavior;
}

#[enum_dispatch(LayoutApi)]
#[derive(Debug, Copy, Clone)]
pub(crate) enum UxLayout {
    ChatLayout,
    MenuLayout,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct UxContext {
    /// the type of the Ux defined here
    pub layout: UxLayout,
    /// what prediction engine is being used
    pub predictor: Option<String::<64>>,
    /// a putative human-readable name given to the context.
    /// Passed to the TokenManager to compute a trust level; add the app's name to tokens.rs EXPECTED_BOOT_CONTEXTS if you want this to succeed.
    pub app_token: [u32; 4], // shared with the app, can be used for other auths to other servers (e.g. audio codec)
    /// a token associated with the UxContext, but private to the GAM (not shared with the app). [currently no use for this, just seems like a good idea...]
    pub gam_token: [u32; 4],
    /// sets a trust level, 255 is the highest (status bar); 254 is a boot-validated context. Less trusted content canvases default to 127.
    pub trust_level: u8,
    /// set to true if keyboard vibrate is turned on
    pub vibe: bool,

    /// CID to send ContextEvents
    pub listener: xous::CID,
    /// opcode ID for redraw
    pub redraw_id: u32,
    /// opcode ID for GotInput Line
    pub gotinput_id: Option<u32>,
    /// opcode ID for raw keystroke data
    pub rawkeys_id: Option<u32>,
    /// opcode ID for AudioFrame
    pub audioframe_id: Option<u32>,
}
const MAX_UX_CONTEXTS: usize = 4;
pub(crate) const MAX_CANVASES: usize = 32;
const BOOT_APP_NAME: &'static str = "shellchat"; // this is the app to display on boot
pub const MAIN_MENU_NAME: &'static str = "main menu";
const BOOT_CONTEXT_TRUSTLEVEL: u8 = 254;

/*
  For now, app focus from menus is cooperative (menu items must relinquish focus).
  However, later on, I think it would be good to implement a press-hold to feature to
  swap focus in case of an app hang failure. This feature would probably be best done
  by adding a hook to the keyboard manager to look for a press-hold on the "select" key
  and then sending a message to the registered listener about the issue.
*/
struct ContextManager {
    tm: TokenManager,
    contexts: [Option<UxContext>; MAX_UX_CONTEXTS],
    focused_context: Option<[u32; 4]>, // app_token of the app that has I/O focus, if any
    last_context: Option<[u32; 4]>, // previously focused context, if any
    imef: ime_plugin_api::ImeFrontEnd,
    imef_active: bool,
    kbd: keyboard::Keyboard,
}
impl ContextManager {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        // hook the keyboard event server and have it forward keys to our local main loop
        let kbd = keyboard::Keyboard::new(&xns).expect("can't connect to KBD");
        kbd.register_listener(api::SERVER_NAME_GAM, Some(Opcode::KeyboardEvent as u32), None).expect("couldn't register for keyboard events");

        info!("acquiring connection to IMEF...");
        let mut imef = ime_plugin_api::ImeFrontEnd::new(&xns).expect("Couldn't connect to IME front end");
        imef.hook_listener_callback(imef_cb).expect("couldn't request events from IMEF");
        ContextManager {
            tm: TokenManager::new(&xns),
            contexts: [None; MAX_UX_CONTEXTS],
            focused_context: None,
            last_context: None,
            imef,
            imef_active: false,
            kbd,
        }
    }
    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        self.tm.claim_token(name)
    }
    pub(crate) fn allow_untrusted_code(&self) -> bool {
        self.tm.allow_untrusted_code()
    }
    pub(crate) fn is_token_valid(&self, token: [u32; 4]) -> bool {
        self.tm.is_token_valid(token)
    }
    pub(crate) fn register(&mut self,
                gfx: &graphics_server::Gfx,
                trng: &trng::Trng,
                status_canvas: &Canvas,
                canvases: &mut FnvIndexMap<Gid, Canvas, MAX_CANVASES>,
                trust_level: u8,
                registration: UxRegistration)
            -> Option<[u32; 4]> {
        let maybe_token = self.tm.claim_token(registration.app_name.as_str().unwrap());
        let mut found_slot = false;
        if let Some(token) = maybe_token {
            match registration.ux_type {
                UxType::Chat => {
                    let mut chatlayout = ChatLayout::init(&gfx, &trng,
                        trust_level, &status_canvas, canvases).expect("couldn't create chat layout");
                    // default to off-screen for all layouts
                    chatlayout.set_visibility_state(false, canvases);
                        let ux_context = UxContext {
                        layout: UxLayout::ChatLayout(chatlayout),
                        predictor: registration.predictor,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        trust_level,
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: registration.gotinput_id,
                        audioframe_id: registration.audioframe_id,
                        rawkeys_id: None,
                        vibe: false,
                    };
                    for maybe_context in self.contexts.iter_mut() {
                        if maybe_context.is_none() {
                            *maybe_context = Some(ux_context);
                            found_slot = true;
                            break;
                        }
                    }
                },
                UxType::Menu => {
                    let mut menulayout = MenuLayout::init(&gfx, &trng,
                        trust_level, canvases).expect("couldn't create menu layout");
                    // default to off-screen for all layouts
                    menulayout.set_visibility_state(false, canvases);
                    log::debug!("debug layout: {:?}", menulayout);
                    let ux_context = UxContext {
                        layout: UxLayout::MenuLayout(menulayout),
                        predictor: None,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        trust_level,
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: None,
                        audioframe_id: None,
                        rawkeys_id: registration.rawkeys_id,
                        vibe: false,
                    };
                    for maybe_context in self.contexts.iter_mut() {
                        if maybe_context.is_none() {
                            *maybe_context = Some(ux_context);
                            found_slot = true;
                            break;
                        }
                    }
                }
            }
        } else {
            // at the moment, we don't allow contexts that are not part of the boot set.
            // however, if later on we want to allow those, here is where we would then allocate these
            // contexts and assign them a lower trust level
            return None;
        }

        if found_slot {
            maybe_token
        } else {
            None
        }
    }
    pub(crate) fn get_content_canvas(&self, token: [u32; 4]) -> Option<Gid> {
        for maybe_context in self.contexts.iter() {
            if let Some(context) = maybe_context {
                if context.app_token == token {
                    return Some(context.layout.get_content_canvas());
                }
            }
        }
        None
    }
    pub(crate) fn set_canvas_height(&mut self,
        gfx: &graphics_server::Gfx,
        gam_token: [u32; 4],
        new_height: i16,
        status_canvas: &Canvas,
        canvases: &mut FnvIndexMap<Gid, Canvas, MAX_CANVASES>) -> Option<Point> {

        for maybe_context in self.contexts.iter_mut() {
            if let Some(context) = maybe_context {
                if context.gam_token == gam_token {
                    let result = context.layout.resize_height(gfx, new_height, status_canvas, canvases).expect("couldn't adjust height of active Ux context");
                    return Some(result)
                }
            }
        }
        None
    }
    fn get_context_by_token_mut(&'_ mut self, token: [u32; 4]) -> Option<&'_ mut UxContext> {
        for maybe_context in self.contexts.iter_mut() {
            if let Some(context) = maybe_context {
                if context.app_token == token {
                    return Some(context)
                }
            }
        }
        None
    }
    fn get_context_by_token(&'_ self, token: [u32; 4]) -> Option<&'_ UxContext> {
        for maybe_context in self.contexts.iter() {
            if let Some(context) = maybe_context {
                if context.app_token == token {
                    return Some(context)
                }
            }
        }
        None
    }
    pub(crate) fn activate(&mut self,
        gfx: &graphics_server::Gfx,
        canvases: &mut FnvIndexMap<Gid, Canvas, MAX_CANVASES>,
        token: [u32; 4],
        clear: bool,
    ) {
        let mut leaving_visibility: bool = false;
        {
            // using a temp copy of the old focus, check if we need to update any visibility state
            let maybe_leaving_focused_context = if self.focused_context.is_some() {
                if let Some(old_context) = self.get_context_by_token(self.focused_context.unwrap()) {
                    Some(old_context.clone())
                } else {
                    None
                }
            } else {
                None
            };
            let maybe_new_focus = self.get_context_by_token_mut(token);
            if let Some(context) = maybe_new_focus {
                if let Some(leaving_focused_context) = maybe_leaving_focused_context {
                    if token != leaving_focused_context.app_token {
                            if (context.layout.behavior()                 == LayoutBehavior::Alert) &&
                            (leaving_focused_context.layout.behavior() == LayoutBehavior::Alert) {
                                context.layout.set_visibility_state(true, canvases);
                                //leaving_focused_context.layout.set_visibility_state(false, canvases);
                                leaving_visibility = false;
                        } else if (context.layout.behavior()                 == LayoutBehavior::App) &&
                                (leaving_focused_context.layout.behavior() == LayoutBehavior::App) {
                                context.layout.set_visibility_state(true, canvases);
                                //leaving_focused_context.layout.set_visibility_state(false, canvases);
                                leaving_visibility = false;
                        } else if (context.layout.behavior()                 == LayoutBehavior::Alert) &&
                                (leaving_focused_context.layout.behavior() == LayoutBehavior::App) {
                                context.layout.set_visibility_state(true, canvases);
                                //leaving_focused_context.layout.set_visibility_state(true, canvases);
                                leaving_visibility = true;
                        } else if (context.layout.behavior()                 == LayoutBehavior::App) &&
                                (leaving_focused_context.layout.behavior() == LayoutBehavior::Alert) {
                                context.layout.set_visibility_state(true, canvases);
                                //leaving_focused_context.layout.set_visibility_state(false, canvases);
                                leaving_visibility = false;
                        }
                    }
                } else {
                    // there was no current focus, just make the activation visible
                    log::debug!("setting first-time visibility to context {:?}", token);
                    context.layout.set_visibility_state(true, canvases);
                }
            }
        }
        {
            // let all the previous operations go out of scope, so we can "check out" the old copy and modify it
            if self.focused_context.is_some() {
                if let Some(old_context) = self.get_context_by_token_mut(self.focused_context.unwrap()) {
                    old_context.layout.set_visibility_state(leaving_visibility, canvases);
                }
            }
        }
        {
            // now re-check-out the new context and finalize things
            let maybe_new_focus = self.get_context_by_token(token);
            if let Some(context) = maybe_new_focus {
                if context.predictor.is_some() {
                    // only hook up the IMEF if a predictor is selected for this context
                    let descriptor = ImefDescriptor {
                        input_canvas: context.layout.get_input_canvas(),
                        prediction_canvas: context.layout.get_prediction_canvas(),
                        predictor: context.predictor,
                        token: context.gam_token,
                    };
                    self.imef.connect_backend(descriptor).expect("couldn't connect IMEF to the current app");
                    self.imef_active = true;
                } else {
                    self.imef_active = false;
                }

                // now recompute the drawability of canvases, based on on-screen visibility and trust state
                let screensize = gfx.screen_size().expect("Couldn't get screen size");
                *canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));
            }
        }
        {
            // now re-check-out the new context and finalize things
            let maybe_new_focus = self.get_context_by_token(token);
            if let Some(context) = maybe_new_focus {
                if clear {
                    context.layout.clear(gfx, canvases).expect("can't clear on context activation");
                }
                // now update the IMEF area, since we're initialized
                // note: we may need to skip this call if the context does not utilize a predictor...
                if context.predictor.is_some() {
                    self.imef.redraw().unwrap();
                }

                // revert the keyboard vibe state
                self.kbd.set_vibe(context.vibe).expect("couldn't restore keyboard vibe");

                let last_token = context.app_token;
                self.last_context = self.focused_context;
                self.focused_context = Some(last_token);
                self.redraw().expect("couldn't redraw the currently focused app");
            }
        }
    }
    pub(crate) fn revert_focus(&mut self,
        gfx: &graphics_server::Gfx,
        canvases: &mut FnvIndexMap<Gid, Canvas, MAX_CANVASES>,
    ) {
        if let Some(last) = self.last_context {
            self.activate(gfx, canvases, last, false);
        }
    }
    pub(crate) fn redraw(&self) -> Result<(), xous::Error> { // redraws the currently focused context
        if let Some(token) = self.focused_app() {
            for maybe_context in self.contexts.iter() {
                if let Some(context) = maybe_context {
                    if token == context.app_token {
                        return xous::send_message(context.listener,
                            xous::Message::new_scalar(context.redraw_id as usize, 0, 0, 0, 0)
                        ).map(|_| ())
                    }
                }
            }
        } else {
            return Err(xous::Error::UseBeforeInit)
        }
        Err(xous::Error::ServerNotFound)
    }
    pub(crate) fn find_app_token_by_name(&self, name: &str) -> Option<[u32; 4]> {
        self.tm.find_token(name)
    }
    pub(crate) fn focused_app(&self) -> Option<[u32; 4]> {
        self.focused_context
    }
    pub(crate) fn forward_input(&self, input: String::<4000>) -> Result<(), xous::Error> {
        if let Some(token) = self.focused_app() {
            for maybe_context in self.contexts.iter() {
                if let Some(context) = maybe_context {
                    if token == context.app_token {
                        if let Some(input_op) = context.gotinput_id {
                            let buf = Buffer::into_buf(input).or(Err(xous::Error::InternalError)).unwrap();
                            return buf.send(context.listener, input_op).map(|_| ())
                        }
                    }
                }
            }
        } else {
            return Err(xous::Error::UseBeforeInit)
        }
        Err(xous::Error::ServerNotFound)
    }
    pub(crate) fn key_event(&mut self, keys: [char; 4],
        gfx: &graphics_server::Gfx,
        canvases: &mut FnvIndexMap<Gid, Canvas, MAX_CANVASES>,
    ) {
        // only pop up the menu if the primary key hit is the menu key (search just the first entry of keys); reject multi-key hits
        if keys[0] == '∴' {
            if let Some(menu_token) = self.find_app_token_by_name(MAIN_MENU_NAME) {
                // set the menu to the active context
                self.activate(gfx, canvases, menu_token, false);
            }
        }

        if self.imef_active {
            // use the IMEF
            self.imef.send_keyevent(keys).expect("couldn't send keys to the IMEF");
        } else {
            // forward the keyboard hits without any IME to the current context
            log::debug!("forwarding raw key event");
            if let Some(context) = self.focused_context() {
                if let Some(rawkeys_id) = context.rawkeys_id {
                    xous::send_message(context.listener,
                        xous::Message::new_scalar(rawkeys_id as usize,
                        keys[0] as u32 as usize,
                        keys[1] as u32 as usize,
                        keys[2] as u32 as usize,
                        keys[3] as u32 as usize,
                    )).expect("couldn't forward raw keys onto context listener");
                }
            }
        }
    }

    fn focused_context(&'_ self) -> Option<&'_ UxContext> {
        if let Some(focus) = self.focused_app() {
            self.get_context_by_token(focus)
        } else {
            None
        }
    }
    fn focused_context_mut(&'_ mut self) -> Option<&'_ mut UxContext> {
        if let Some(focus) = self.focused_app() {
            self.get_context_by_token_mut(focus)
        } else {
            None
        }
    }
    pub(crate) fn set_audio_op(&mut self, audio_op: SetAudioOpcode) {
        if let Some(context) = self.focused_context_mut() {
            (*context).audioframe_id = Some(audio_op.opcode);
        }
    }
    pub(crate) fn vibe(&mut self, set_vibe: bool) {
        self.kbd.set_vibe(set_vibe).expect("couldn't set vibe on keyboard");
        if let Some(context) = self.focused_context_mut() {
            (*context).vibe = set_vibe;
        }
    }
}


static CB_TO_MAIN_CONN: AtomicU32 = AtomicU32::new(0);
fn imef_cb(s: String::<4000>) {
    if CB_TO_MAIN_CONN.load(Ordering::Relaxed) != 0 {
        let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
        let buf = xous_ipc::Buffer::into_buf(s).or(Err(xous::Error::InternalError)).unwrap();
        buf.lend(cb_to_main_conn, Opcode::InputLine.to_u32().unwrap()).unwrap();
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed; this is a gateway server
    let gam_sid = xns.register_name(api::SERVER_NAME_GAM, None).expect("can't register server");
    CB_TO_MAIN_CONN.store(xous::connect(gam_sid).unwrap(), Ordering::Relaxed);
    log::trace!("starting up...");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let gfx = graphics_server::Gfx::new(&xns).expect("can't connect to GFX");
    let trng = trng::Trng::new(&xns).expect("can't connect to TRNG");

    let mut context_mgr = ContextManager::new(&xns);

    // a map of canvases accessable by Gid
    let mut canvases: FnvIndexMap<Gid, Canvas, MAX_CANVASES> = FnvIndexMap::new();

    let screensize = gfx.screen_size().expect("Couldn't get screen size");
    let small_height: i16 = gfx.glyph_height_hint(GlyphStyle::Small).expect("couldn't get glyph height") as i16;

    // the status canvas is special -- there can only be one, and it is ultimately trusted
    let status_canvas = Canvas::new(
        Rectangle::new_coords(0, 0, screensize.x, small_height * 2),
        255, &trng, None
    ).expect("couldn't create status canvas");
    canvases.insert(status_canvas.gid(), status_canvas).expect("can't store status canvus");
    canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));

    // make a thread to manage the status bar -- this needs to start late, after the IMEF and most other things are initialized
    // the status bar is a trusted element managed by the OS, and we are chosing to domicile this in the GAM process for now
    let status_gid = status_canvas.gid().gid();
    log::trace!("starting status thread with gid {:?}", status_gid);
    xous::create_thread_4(status_thread, status_gid[0] as _, status_gid[1] as _, status_gid[2] as _, status_gid[3] as _).expect("couldn't create status thread");

    // make a thread to manage the main menu.
    log::trace!("starting menu thread");
    xous::create_thread_0(main_menu_thread).expect("couldn't create menu thread");

    let mut powerdown_requested = false;
    let mut last_time: u64 = ticktimer.elapsed_ms();
    log::trace!("entering main loop");
    loop {
        let mut msg = xous::receive_message(gam_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::ClearCanvas) => {
                msg_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let gid = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                            gfx.draw_rectangle(rect).expect("can't clear canvas");
                        },
                        None => info!("attempt to clear bogus canvas, ignored."),
                    }
                });
            }
            Some(Opcode::GetCanvasBounds) => {
                msg_blocking_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let gid = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.normalize(); // normalize to 0,0 coordinates
                            log::debug!("getcanvasbounds: {:?}", rect);
                            xous::return_scalar2(msg.sender,
                                rect.tl.into(),
                                rect.br.into(),
                            ).expect("couldn't return canvas bounds");
                        },
                        None => info!("attempt to get bounds on bogus canvas gid {:?}, {:?} ignored.", gid, msg),
                    }
                })
            }
            Some(Opcode::PowerDownRequest) => {
                msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    powerdown_requested = true;
                    gfx.draw_sleepscreen().expect("couldn't draw sleep screen");
                    // a screen flush is part of the draw_sleepscreen abstraction
                    xous::return_scalar(msg.sender, 1).expect("couldn't confirm power down UI request");
                })
            }
            Some(Opcode::ShipModeBlankRequest) => {
                msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    powerdown_requested = true;
                    gfx.draw_rectangle(Rectangle::new_with_style(Point::new(0,0), screensize, DrawStyle::new(PixelColor::Light, PixelColor::Light, 0))).expect("couldn't clear screen");
                    gfx.flush().expect("couldn't refresh blank screen");
                    xous::return_scalar(msg.sender, 1).expect("couldn't confirm power down UI request");
                })
            }
            Some(Opcode::Redraw) => {
                msg_scalar_unpack!(msg, _, _, _, _, {
                    log::trace!("redraw message received");
                    if powerdown_requested {
                        continue; // don't allow any redraws if a powerdown is requested
                    }
                    let elapsed_time = ticktimer.elapsed_ms();
                    if elapsed_time - last_time > 33 {  // rate limit updates, no point in going faster than the eye can see
                        last_time = elapsed_time;

                        deface(&gfx, &mut canvases);
                        log::trace!("flushing...");
                        gfx.flush().expect("couldn't flush buffer to screen");
                        /* // this throws errors right now because deface() doesn't work.
                        for (_, c) in canvases.iter_mut() {
                            c.do_flushed();
                        }*/
                    }
                })
            }
            Some(Opcode::RenderTextView) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tv = buffer.to_original::<TextView, _>().unwrap();
                log::trace!("rendertextview {:?}", tv);
                match tv.get_op() {
                    TextOp::Nop => (),
                    TextOp::Render | TextOp::ComputeBounds => {
                        if tv.invert & tv.token.is_some() {
                            // an inverted text can only be made by secure processes. check that it has a valid token.
                            if !context_mgr.is_token_valid(tv.token.unwrap()) {
                                log::error!("Attempt to draw inverted text without valid credentials. Aborting.");
                                continue;
                            }
                        }

                        log::trace!("render request for {:?}", tv);
                        if tv.get_op() == TextOp::ComputeBounds {
                            tv.dry_run = true;
                        } else {
                            tv.dry_run = false;
                        }

                        if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                            // if we're requesting inverted text, this better be a "trusted canvas"
                            if tv.invert & (canvas.trust_level() < BOOT_CONTEXT_TRUSTLEVEL) {
                                log::error!("Attempt to draw inverted text without sufficient trust level. Aborting.");
                                continue;
                            }
                            // first, figure out if we should even be drawing to this canvas.
                            if canvas.is_drawable() {
                                // set the clip rectangle according to the canvas' location
                                tv.clip_rect = Some(canvas.clip_rect().into());

                                // you have to clone the tv object, because if you don't the same block of
                                // memory gets passed on to the graphics_server(). Which is efficient, but,
                                // the call will automatically Drop() the memory, which causes a panic when
                                // this routine returns.
                                let mut tv_clone = tv.clone();
                                // issue the draw command
                                gfx.draw_textview(&mut tv_clone).expect("text view draw could not complete.");
                                // copy back the fields that we want to be mutable
                                log::trace!("got computed cursor of {:?}", tv_clone.cursor);
                                tv.cursor = tv_clone.cursor;
                                tv.bounds_computed = tv_clone.bounds_computed;

                                let ret = api::Return::RenderReturn(tv);
                                buffer.replace(ret).unwrap();
                                canvas.do_drawn().expect("couldn't set canvas to drawn");
                            } else {
                                info!("attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored. {:?}", tv);
                            }
                        } else {
                            info!("bogus GID {:?} in TextView {}, not doing anything in response to draw request.", tv.get_canvas_gid(), tv.text);
                            // silently fail if a bogus Gid is given???
                        }
                    },
                };
            }
            Some(Opcode::SetCanvasBounds) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut cb = buffer.to_original::<SetCanvasBoundsRequest, _>().unwrap();
                log::debug!("SetCanvasBoundsRequest {:?}", cb);

                let granted = context_mgr.set_canvas_height(&gfx, cb.token, cb.requested.y, &&status_canvas, &mut canvases);
                if granted.is_some() {
                    // recompute the canvas orders based on the new layout
                    canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));
                    context_mgr.redraw().expect("can't redraw after new canvas bounds");
                }
                cb.granted = granted;
                let ret = api::Return::SetCanvasBoundsReturn(cb);
                log::debug!("returning {:?}", cb);
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RequestContentCanvas) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let req = buffer.to_original::<[u32; 4], _>().unwrap();
                log::debug!("RequestContentCanvas {:?}", req);

                let ret = api::Return::ContentCanvasReturn(context_mgr.get_content_canvas(req));
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RenderObject) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let obj = buffer.to_original::<GamObject, _>().unwrap();
                log::debug!("renderobject {:?}", obj);
                if let Some(canvas) = canvases.get_mut(&obj.canvas) {
                    // first, figure out if we should even be drawing to this canvas.
                    if canvas.is_drawable() {
                        match obj.obj {
                            GamObjectType::Line(mut line) => {
                                line.translate(canvas.clip_rect().tl);
                                line.translate(canvas.pan_offset());
                                gfx.draw_line_clipped(
                                    line,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw line");
                            },
                            GamObjectType::Circ(mut circ) => {
                                circ.translate(canvas.clip_rect().tl);
                                circ.translate(canvas.pan_offset());
                                gfx.draw_circle_clipped(
                                    circ,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw circle");
                            },
                            GamObjectType::Rect(mut rect) => {
                                rect.translate(canvas.clip_rect().tl);
                                rect.translate(canvas.pan_offset());
                                gfx.draw_rectangle_clipped(
                                    rect,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw rectangle");
                            },
                            GamObjectType::RoundRect(mut rr) => {
                                rr.translate(canvas.clip_rect().tl);
                                rr.translate(canvas.pan_offset());
                                gfx.draw_rounded_rectangle_clipped(
                                    rr,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw rounded rectangle");
                            }
                        }
                        canvas.do_drawn().expect("couldn't set canvas to drawn");
                    } else {
                        info!("attempt to draw Object on non-drawable canvas. Not fatal, but request ignored.");
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
                log::trace!("leaving RenderObject");
            }
            Some(Opcode::ClaimToken) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tokenclaim = buffer.to_original::<TokenClaim, _>().unwrap();
                tokenclaim.token = context_mgr.claim_token(tokenclaim.name.as_str().unwrap());
                buffer.replace(tokenclaim).unwrap();
            },
            Some(Opcode::TrustedInitDone) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if context_mgr.allow_untrusted_code() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::RegisterUx) => {
                let mut buffer = unsafe{ Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let registration = buffer.to_original::<UxRegistration, _>().unwrap();

                // note that we are currently assigning all Ux registrations a trust level consistent with a boot context (ultimately trusted)
                // this needs to be modified later on once we allow post-boot apps to be created
                let token = context_mgr.register(&gfx, &trng, &status_canvas, &mut canvases,
                    BOOT_CONTEXT_TRUSTLEVEL, registration);

                // compute what canvases are drawable
                // this _replaces_ the original canvas structure, to avoid complications of tracking mutable references through compound data structures
                // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
                canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));

                buffer.replace(Return::UxToken(token)).unwrap();
            },
            Some(Opcode::SetAudioOpcode) => {
                let buffer = unsafe{ Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let audio_op = buffer.to_original::<SetAudioOpcode, _>().unwrap();
                context_mgr.set_audio_op(audio_op);
            },
            Some(Opcode::InputLine) => {
                // receive the keyboard input and pass it on to the context with focus
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let inputline = buffer.to_original::<String::<4000>, _>().unwrap();
                log::debug!("received input line, forwarding on... {}", inputline);
                context_mgr.forward_input(inputline).expect("couldn't forward input line to focused app");
                log::debug!("returned from forward_input");
            },
            Some(Opcode::KeyboardEvent) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                context_mgr.key_event(keys, &gfx, &mut canvases);
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, ena, _,  _,  _, {
                if ena != 0 { context_mgr.vibe(true) }
                else { context_mgr.vibe(false) }
            }),
            Some(Opcode::RevertFocus) => {
                context_mgr.revert_focus(&gfx, &mut canvases);
            },
            Some(Opcode::RequestFocus) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                // TODO: add some limitations around who can request focus
                // for now, it's the boot set so we just trust the requestor
                context_mgr.activate(&gfx, &mut canvases, [t0 as u32, t1 as u32, t2 as u32, t3 as u32], true);

                // this is a blocking scalar, so return /something/ so we know to move on
                xous::return_scalar(msg.sender, 1).expect("couldn't confirm focus activation");
            }),
            Some(Opcode::Quit) => break,
            None => {log::error!("unhandled message {:?}", msg);}
        }
        /*
        // if we don't have a focused app, try and find the default boot app and bring it to focus.
        if context_mgr.focused_app().is_none() {
            if let Some(shellchat_token) = context_mgr.find_app_token_by_name(BOOT_APP_NAME) {
                context_mgr.activate(&gfx, &mut canvases, shellchat_token, true);
            }
        }*/
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(gam_sid).unwrap();
    xous::destroy_server(gam_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
