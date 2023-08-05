use crate::*;
use graphics_server::*;
use ime_plugin_api::{ImeFrontEndApi, ImefDescriptor};
use xous_ipc::{Buffer, String};
use crate::api::Opcode;
use gam::MAIN_MENU_NAME;

use log::info;
use std::collections::HashMap;
use enum_dispatch::enum_dispatch;

// todo:
// - add auth tokens to audio streams, so less trusted processes can make direct connections to the codec and reduce latency

pub (crate) const MISC_CONTEXT_DEFAULT_TRUST: u8 = 127;

#[derive(PartialEq, Eq)]
pub(crate) enum LayoutBehavior {
    /// a layout that can render over others, takes focus, and only dismissed if explicitly dismissed
    Alert,
    /// a layout that assumes it has the full screen and is the primary content when visible
    App,
}

#[enum_dispatch]
pub(crate) trait LayoutApi {
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut HashMap<Gid, Canvas>) -> Result<(), xous::Error>;
    // for Chats, this resizes the height of the input area; for menus, it resizes the overall height
    fn resize_height(&mut self, gfx: &graphics_server::Gfx, new_height: i16, status_cliprect: &Rectangle, canvases: &mut HashMap<Gid, Canvas>) -> Result<Point, xous::Error>;
    fn get_gids(&self) -> Vec<GidRecord>;
    //fn get_input_canvas(&self) -> Option<Gid> { None }
    //fn get_prediction_canvas(&self) -> Option<Gid> { None }
    //fn get_content_canvas(&self) -> Gid; // layouts always have a content canvas
    // when the argument is true, the context is moved "onscreen" by moving the canvases into the screen clipping rectangle
    // when false, the context is moved "offscreen" by moving the canvases outside the screen clipping rectangle
    // note that this visibility state is an independent variable from the trust level draw-ability
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut HashMap<Gid, Canvas>);
    fn behavior(&self) -> LayoutBehavior;
}

#[enum_dispatch(LayoutApi)]
#[derive(Debug, Copy, Clone)]
pub(crate) enum UxLayout {
    ChatLayout,
    MenuLayout,
    ModalLayout,
    Framebuffer,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct UxContext {
    /// the type of the Ux defined here
    pub layout: UxLayout,
    /// what prediction engine is being used
    pub predictor: Option<String::<64>>,
    /// a putative human-readable name given to the context. The name itself is stored in the TokenManager, not in this struct.
    /// Passed to the TokenManager to compute a trust level; add the app's name to tokens.rs EXPECTED_BOOT_CONTEXTS if you want this to succeed.
    pub app_token: [u32; 4], // shared with the app, can be used for other auths to other servers (e.g. audio codec)
    /// a token associated with the UxContext, but private to the GAM (not shared with the app). (used by predictor to set API tokens)
    pub gam_token: [u32; 4],
    /// set to true if keyboard vibrate is turned on
    pub vibe: bool,
    /// API token for the predictor. Allows our prediction history to be shown only when our context is active.
    pub pred_token: Option<[u32; 4]>,

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
    /// opcode ID for focus change
    pub focuschange_id: Option<u32>,
    /// sets the behavior of the IMEF
    pub imef_menu_mode: bool,
}
pub(crate) const BOOT_CONTEXT_TRUSTLEVEL: u8 = 254;

/*
  For now, app focus from menus is cooperative (menu items must relinquish focus).
  However, later on, I think it would be good to implement a press-hold to feature to
  swap focus in case of an app hang failure. This feature would probably be best done
  by adding a hook to the keyboard manager to look for a press-hold on the "select" key
  and then sending a message to the registered listener about the issue.
*/
pub(crate) struct ContextManager {
    tm: TokenManager,
    contexts: HashMap::<[u32; 4], UxContext>,
    focused_context: Option<[u32; 4]>, // app_token of the app that has I/O focus, if any
    last_context: Option<[u32; 4]>, // previously focused context, if any
    context_stack: Vec::<[u32; 4]>,
    imef: ime_plugin_api::ImeFrontEnd,
    imef_active: bool,
    kbd: keyboard::Keyboard,
    main_menu_app_token: Option<[u32; 4]>, // app_token of the main menu, if it has been registered
    /// for internal generation of deface states
    pub trng: trng::Trng,
    tt: ticktimer_server::Ticktimer,
    /// used to suppress the main menu from activating until the boot PIN has been requested
    allow_mainmenu: bool,
}
impl ContextManager {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        // hook the keyboard event server and have it forward keys to our local main loop
        let kbd = keyboard::Keyboard::new(&xns).expect("can't connect to KBD");
        kbd.register_listener(crate::api::SERVER_NAME_GAM, Opcode::KeyboardEvent as usize);

        info!("acquiring connection to IMEF...");
        let mut imef = ime_plugin_api::ImeFrontEnd::new(&xns).expect("Couldn't connect to IME front end");
        imef.hook_listener_callback(imef_cb).expect("couldn't request events from IMEF");
        ContextManager {
            tm: TokenManager::new(&xns),
            contexts: HashMap::new(),
            context_stack: Vec::new(),
            focused_context: None,
            last_context: None,
            imef,
            imef_active: false,
            kbd,
            main_menu_app_token: None,
            trng: trng::Trng::new(&xns).expect("couldn't connect to trng"),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            allow_mainmenu: false,
        }
    }
    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        self.tm.claim_token(name)
    }
    pub(crate) fn register_name(&mut self, name: &str, auth_token: &[u32; 4]) {
	self.tm.register_name(name, auth_token);
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
                status_cliprect: &Rectangle,
                canvases: &mut HashMap<Gid, Canvas>,
                registration: UxRegistration)
            -> Option<[u32; 4]> {
        let maybe_token = self.tm.claim_token(registration.app_name.as_str().unwrap());
        if let Some(token) = maybe_token {
            match registration.ux_type {
                UxType::Chat => {
                    let mut chatlayout = ChatLayout::init(&gfx, &trng,
                        status_cliprect, canvases).expect("couldn't create chat layout");
                    // default to off-screen for all layouts
                    chatlayout.set_visibility_state(false, canvases);
                        let ux_context = UxContext {
                        layout: UxLayout::ChatLayout(chatlayout),
                        predictor: registration.predictor,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: registration.gotinput_id,
                        audioframe_id: registration.audioframe_id,
                        focuschange_id: registration.focuschange_id,
                        rawkeys_id: None,
                        vibe: false,
                        imef_menu_mode: false,
                        // this gets initialized on the first attempt to change predictors, not here
                        pred_token: None,
                    };
                    self.contexts.insert(token, ux_context);
                },
                UxType::Menu => {
                    let mut menulayout = MenuLayout::init(&gfx, &trng,
                        canvases).expect("couldn't create menu layout");
                    // default to off-screen for all layouts
                    menulayout.set_visibility_state(false, canvases);
                    log::debug!("debug menu layout: {:?}", menulayout);
                    let ux_context = UxContext {
                        layout: UxLayout::MenuLayout(menulayout),
                        predictor: None,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: None,
                        audioframe_id: None,
                        focuschange_id: registration.focuschange_id,
                        rawkeys_id: registration.rawkeys_id,
                        vibe: false,
                        imef_menu_mode: false,
                        pred_token: None,
                    };

                    if registration.app_name.as_str().unwrap() == MAIN_MENU_NAME {
                        log::debug!("main menu found and registered!");
                        assert!(self.main_menu_app_token == None, "attempt to double-register main menu handler, this should never happen.");
                        self.main_menu_app_token = Some(token);
                    }
                    self.contexts.insert(token, ux_context);
                }
                UxType::Modal => {
                    let mut modallayout = ModalLayout::init(&gfx, &trng,
                        canvases).expect("couldn't create modal layout");
                    // default to off-screen for all layouts
                    modallayout.set_visibility_state(false, canvases);
                    log::debug!("debug modal layout: {:?}", modallayout);
                    let ux_context = UxContext {
                        layout: UxLayout::ModalLayout(modallayout),
                        predictor: None,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: None,
                        audioframe_id: None,
                        focuschange_id: registration.focuschange_id,
                        rawkeys_id: registration.rawkeys_id,
                        vibe: false,
                        imef_menu_mode: false,
                        pred_token: None,
                    };
                    self.contexts.insert(token, ux_context);
                    // this check gives permissions to password boxes to render inverted text
                    if registration.app_name.as_str().unwrap() == gam::ROOTKEY_MODAL_NAME
                    || registration.app_name.as_str().unwrap() == gam::PDDB_MODAL_NAME {
                        if !self.set_context_trust_level(token, BOOT_CONTEXT_TRUSTLEVEL - 1, canvases) {
                            log::error!("Couldn't set password box trust levels to fully trusted");
                        }
                    }
                }
                UxType::Framebuffer => {
                    let mut raw_fb = Framebuffer::init(&gfx, &trng,
                        &status_cliprect, canvases).expect("couldn't create raw fb layout");
                    raw_fb.set_visibility_state(false, canvases);
                    log::debug!("debug raw fb layout: {:?}", raw_fb);
                    let ux_context = UxContext {
                        layout: UxLayout::Framebuffer(raw_fb),
                        predictor: None,
                        app_token: token,
                        gam_token: [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), ],
                        listener: xous::connect(xous::SID::from_array(registration.listener)).unwrap(),
                        redraw_id: registration.redraw_id,
                        gotinput_id: None,
                        audioframe_id: None,
                        focuschange_id: registration.focuschange_id,
                        rawkeys_id: registration.rawkeys_id,
                        vibe: false,
                        imef_menu_mode: false,
                        pred_token: None,
                    };
                    self.contexts.insert(token, ux_context);
                }
            }
        } else {
            // at the moment, we don't allow contexts that are not part of the boot set.
            // however, if later on we want to allow those, here is where we would then allocate these
            // contexts and assign them a lower trust level
            return None;
        }

        maybe_token
    }
    /// private function to set a trust level. should only be done on contexts that we ... trust. returns true if the token is found, false if not.
    fn set_context_trust_level(&self, token: [u32; 4], level: u8, canvases: &mut HashMap<Gid, Canvas>) -> bool {
        if let Some(context) = self.contexts.get(&token) {
            let mut success = true;
            for gr in context.layout.get_gids().iter() {
                if let Some(canvas) = canvases.get_mut(&gr.gid) {
                    canvas.set_trust_level(level);
                } else {
                    success = false;
                }
            }
            success
        } else {
            false
        }
    }
    pub(crate) fn get_content_canvas(&self, token: [u32; 4]) -> Option<Gid> {
        if let Some(context) = self.contexts.get(&token) {
            let gids = context.layout.get_gids();
            if let Some(gr) = gids.iter().filter(|&gr| gr.canvas_type.is_content()).next() {
                Some(gr.gid)
            } else {
                None
            }
        } else {
            None
        }
    }
    pub(crate) fn set_canvas_height(&mut self,
        gfx: &graphics_server::Gfx,
        gam_token: [u32; 4],
        new_height: i16,
        status_cliprect: &Rectangle,
        canvases: &mut HashMap<Gid, Canvas>) -> Option<Point> {

        for context in self.contexts.values_mut() {
            if context.gam_token == gam_token {
                let result = context.layout.resize_height(gfx, new_height, status_cliprect, canvases).expect("couldn't adjust height of active Ux context");
                return Some(result)
            }
        }
        None
    }
    // hmmm...feels wrong to have basically a dupe of the above. Maybe this abstraction needs to be cleaned up a bit.
    pub(crate) fn set_canvas_height_app_token(&mut self,
        gfx: &graphics_server::Gfx,
        app_token: [u32; 4],
        new_height: i16,
        status_cliprect: &Rectangle,
        canvases: &mut HashMap<Gid, Canvas>) -> Option<Point> {

        if let Some(context) = self.contexts.get_mut(&app_token) {
            let result = context.layout.resize_height(gfx, new_height, status_cliprect, canvases).expect("couldn't adjust height of active Ux context");
            Some(result)
        } else {
            None
        }
    }
    fn get_context_by_token_mut(&'_ mut self, token: [u32; 4]) -> Option<&'_ mut UxContext> {
        self.contexts.get_mut(&token)
    }
    fn get_context_by_token(&'_ self, token: [u32; 4]) -> Option<&'_ UxContext> {
        self.contexts.get(&token)
    }
    pub(crate) fn activate(&mut self,
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
        token: [u32; 4],
        clear: bool,
    ) -> Result<(), xous::Error> {
        // log::set_max_level(log::LevelFilter::Trace);
        self.notify_app_switch(token).ok();

        let mut leaving_visibility: bool = false;
        let stack_on_entry = self.context_stack.len();
        {
            // using a temp copy of the old focus, check if we need to update any visibility state
            let maybe_leaving_focused_context = if let Some(focused_token) = self.focused_context {
                log::debug!("leaving {:?}", self.tm.lookup_name(&focused_token));
                if let Some(old_context) = self.get_context_by_token(focused_token) {
                    Some(old_context.clone())
                } else {
                    None
                }
            } else {
                None
            };
            log::debug!("entering {:?}", self.tm.lookup_name(&token));
            let maybe_new_focus = self.get_context_by_token_mut(token);
            log::trace!("resolving visibility rules");
            if let Some(context) = maybe_new_focus {
                if let Some(leaving_focused_context) = maybe_leaving_focused_context {
                    if token != leaving_focused_context.app_token {
                        if  // alert covering an alert
                        (context.layout.behavior()                 == LayoutBehavior::Alert) &&
                        (leaving_focused_context.layout.behavior() == LayoutBehavior::Alert) {
                            // just disallow alerts covering alerts for now...it's first come, first-serve.
                            log::warn!("Disallowing raise of alert over alert");
                            return Err(xous::Error::ShareViolation)
                            // context.layout.set_visibility_state(true, canvases);
                            // leaving_visibility = false;
                        } else if // app covering an app
                        (context.layout.behavior()                 == LayoutBehavior::App) &&
                        (leaving_focused_context.layout.behavior() == LayoutBehavior::App) {
                            log::debug!("resolved: app covering app");
                            context.layout.set_visibility_state(true, canvases);
                            leaving_visibility = false;
                            self.context_stack.pop();
                            self.context_stack.push(token);
                        } else if // alert covering an app
                        (context.layout.behavior()                 == LayoutBehavior::Alert) &&
                        (leaving_focused_context.layout.behavior() == LayoutBehavior::App) {
                            log::debug!("resolved: alert covering app");
                            context.layout.set_visibility_state(true, canvases);
                            leaving_visibility = true;
                            self.context_stack.push(token);
                        } else if // app covering an alert
                        (context.layout.behavior()                 == LayoutBehavior::App) &&
                        (leaving_focused_context.layout.behavior() == LayoutBehavior::Alert) {
                            log::debug!("resolved: app covering alert");
                            context.layout.set_visibility_state(true, canvases);
                            leaving_visibility = false;
                            self.context_stack.pop();
                        }
                    } else {
                        log::warn!("resolved: staying in same context. This is not an expected case.");
                        // this path "shouldn't" happen because the discipline is that every pop-up must
                        // return control back to the main thread (you can't chain pop-ups).
                        // thus, kick out a warning if this happens, but also, try to do something reasonable.
                        self.redraw().expect("couldn't redraw the currently focused app");
                        return Ok(());
                    }
                } else {
                    // there was no current focus, just make the activation visible
                    log::debug!("setting first-time visibility to context {:?}", token);
                    context.layout.set_visibility_state(true, canvases);
                    self.context_stack.push(token);
                }
            }
        }
        log::trace!("hiding old context");
        {
            // let all the previous operations go out of scope, so we can "check out" the old copy and modify it
            if self.focused_context.is_some() {
                // immutable borrow here can't be combined with mutable borrow below
                if let Some(old_context) = self.get_context_by_token_mut(self.focused_context.unwrap()) {
                    old_context.layout.set_visibility_state(leaving_visibility, canvases);
                }
            }
        }
        log::trace!("rewiring IMEF and recomputing canvases");
        {
            // now re-check-out the new context and finalize things
            let maybe_new_focus = self.get_context_by_token(token);
            if let Some(context) = maybe_new_focus {
                if context.predictor.is_some() {
                    // only hook up the IMEF if a predictor is selected for this context
                    let descriptor = ImefDescriptor {
                        input_canvas:
                            if let Some(gr) =
                            context.layout.get_gids().iter().filter(|&gr| gr.canvas_type == CanvasType::ChatInput)
                            .next() {
                                Some(gr.gid)
                            } else {
                                None
                            },
                        prediction_canvas:
                            if let Some(gr) =
                            context.layout.get_gids().iter().filter(|&gr| gr.canvas_type == CanvasType::ChatPreditive)
                            .next() {
                                Some(gr.gid)
                            } else {
                                None
                            },
                        predictor: context.predictor,
                        token: context.gam_token,
                        predictor_token: context.pred_token,
                    };
                    log::debug!("context gam token: {:x?}, pred token: {:x?}", context.gam_token, context.pred_token);
                    self.imef.connect_backend(descriptor).expect("couldn't connect IMEF to the current app");
                    self.imef_active = true;
                } else {
                    self.imef_active = false;
                };

                // now recompute the drawability of canvases, based on on-screen visibility and trust state
                recompute_canvases(canvases);
            }
        }
        log::trace!("foregrounding new context");
        {
            // now re-check-out the new context and finalize things
            let maybe_new_focus = self.get_context_by_token(token);
            if let Some(context) = maybe_new_focus {
                self.imef.set_menu_mode(context.imef_menu_mode).expect("couldn't set menu mode");
                if clear {
                    context.layout.clear(gfx, canvases).expect("can't clear on context activation");
                }
                // note: we may need to skip this call if the context does not utilize a predictor...
                if context.predictor.is_some() {
                    log::debug!("calling IMEF redraw");
                    self.imef.redraw(true).unwrap();
                }

                // revert the keyboard vibe state
                self.kbd.set_vibe(context.vibe).expect("couldn't restore keyboard vibe");

                log::trace!("Raised focus to: {:?}", self.tm.lookup_name(&token));
                let last_token = context.app_token;
                self.last_context = self.focused_context;
                self.focused_context = Some(last_token);
            }
            log::trace!("context stack: {:x?}", self.context_stack);
            if self.context_stack.len() > 1 { // we've now got a stack of contexts, start stashing copies
                log::trace!("stashing");
                gfx.stash(true);
            }
            // run the defacement before we redraw all the canvases
            if deface(gfx, &self.trng, canvases) {
                log::trace!("activate triggered a defacement");
            }
            log::trace!("activate redraw");
            if self.context_stack.len() > 0 && stack_on_entry > 1 {
                if self.context_stack[self.context_stack.len() - 1] == token {
                    // if we're returning to the previous context, just pop the image
                    log::trace!("popping");
                    gfx.pop(true);
                } else {
                    self.redraw().expect("couldn't redraw the currently focused app");
                }
            } else {
                self.redraw().expect("couldn't redraw the currently focused app");
            }
        }
        // log::set_max_level(log::LevelFilter::Info);
        Ok(())
    }
    pub(crate) fn set_pred_api_token(&mut self, at: ApiToken) {
        for context in self.contexts.values_mut() {
            if context.gam_token == at.gam_token {
                log::debug!("setting {:?} token to {:?}", at.gam_token, at.api_token);
                context.pred_token = Some(at.api_token);
                break;
            }
        }
    }
    pub(crate) fn revert_focus(&mut self,
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<(), xous::Error> {
        if let Some(last) = self.last_context {
            self.activate(gfx, canvases, last, false)
        } else {
            Err(xous::Error::UseBeforeInit)
        }
    }
    pub(crate) fn notify_app_switch(&self, new_app_token: [u32; 4]) -> Result<(), xous::Error> {
        log::debug!("Foregrounding {:?} / {:x?}", self.tm.lookup_name(&new_app_token), new_app_token);
        if let Some(current_focus) = self.focused_context {
            if let Some(old_context) = self.get_context_by_token(current_focus) {
                if let Some(focuschange_id) = old_context.focuschange_id {
                    log::debug!("Backgrounding  {:?} (listener {}, id {} / {:x?})", self.tm.lookup_name(&current_focus), old_context.listener, old_context.redraw_id, current_focus);
                    xous::send_message(old_context.listener,
                        xous::Message::new_scalar(focuschange_id as usize, gam::FocusState::Background as usize, 0, 0, 0)
                    ).map(|_| ())?;
                } else {
                    // don't return an error -- this just means that the listener doesn't recognize focus changes. This should not
                    // deprive the later app of a notification that it is coming into focus!
                }
            }
        }

        if let Some(new_context) = self.get_context_by_token(new_app_token) {
            if let Some(focuschange_id) = new_context.focuschange_id {
                log::trace!("Foreground focus change to {:?} ({}, id {} / {:x?})", self.tm.lookup_name(&new_app_token), new_context.listener, new_context.redraw_id, new_app_token);
                xous::send_message(new_context.listener,
                    xous::Message::new_scalar(focuschange_id as usize, gam::FocusState::Foreground as usize, 0, 0, 0)
                ).map(|_| ())?;
            } else {
                return Err(xous::Error::ServerNotFound);
            }
        }
        Ok(())
    }
    pub(crate) fn redraw(&self) -> Result<(), xous::Error> { // redraws the currently focused context
        if let Some(token) = self.focused_app() {
            if let Some(context) = self.contexts.get(&token) {
                log::debug!("redraw msg to {:?} ({}, id {})", self.tm.lookup_name(&token), context.listener, context.redraw_id);
                let ret = match xous::try_send_message(context.listener,
                    xous::Message::new_scalar(context.redraw_id as usize, 0, 0, 0, 0)
                ) {
                    Err(xous::Error::ServerQueueFull) => {
                        log::warn!("server queue full, redraw skipped");
                        Ok(())
                    },
                    Ok(_r) => Ok(()),
                    Err(e) => Err(e),
                };
                // this delay helps ensure that the previously requested UX redraw has time to complete
                // in particular, this helps sequence the case where one modal is erased, and the next one is
                // raised, in quick succession.
                self.tt.sleep_ms(20).unwrap();
                return ret
            }
        } else {
            return Err(xous::Error::UseBeforeInit)
        }
        Err(xous::Error::ServerNotFound)
    }
    pub(crate) fn redraw_imef(&self) -> Result<(), xous::Error> {
        if let Some(context) = self.focused_context() {
            if context.predictor.is_some() {
                log::debug!("calling IMEF redraw");
                self.imef.redraw(true).unwrap();
            }
        }
        Ok(())
    }
    pub(crate) fn find_app_token_by_name(&self, name: &str) -> Option<[u32; 4]> {
        self.tm.find_token(name)
    }
    pub(crate) fn focused_app(&self) -> Option<[u32; 4]> {
        self.focused_context
    }
    pub(crate) fn forward_input(&self, input: String::<4000>) -> Result<(), xous::Error> {
        if let Some(token) = self.focused_app() {
            if let Some(context) = self.contexts.get(&token) {
                if let Some(input_op) = context.gotinput_id {
                    let buf = Buffer::into_buf(input).or(Err(xous::Error::InternalError)).unwrap();
                    return buf.send(context.listener, input_op).map(|_| ())
                }
            }
        } else {
            return Err(xous::Error::UseBeforeInit)
        }
        Err(xous::Error::ServerNotFound)
    }
    pub(crate) fn allow_mainmenu(&mut self) {
        self.allow_mainmenu = true;
    }
    pub(crate) fn key_event(&mut self, keys: [char; 4],
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
    ) {
        // only pop up the menu if the primary key hit is the menu key (search just the first entry of keys); reject multi-key hits
        // only pop up the menu if it isn't already popped up
        if keys[0] == '∴' {
            if let Some(context) = self.get_context_by_token(self.focused_context.unwrap()) {
                if context.layout.behavior() == LayoutBehavior::App {
                    log::info!("allow_mainmenu: {:?}", self.allow_mainmenu);
                    if self.allow_mainmenu {
                        if let Some(menu_token) = self.find_app_token_by_name(MAIN_MENU_NAME) {
                            // set the menu to the active context
                            match self.activate(gfx, canvases, menu_token, false) {
                                Ok(_) => (),
                                Err(_) => log::warn!("Couldn't raise menu, user will have to try again."),
                            }
                            // don't pass the initial key hit back to the menu app, just eat it and return
                            return;
                        }
                    } else {
                        // eat the key and return if it is hit before the boot PIN was entered
                        return;
                    }
                }
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
    pub(crate) fn toggle_menu_mode(&mut self, token: [u32; 4]) {
        if let Some(context) = self.contexts.get_mut(&token) {
            context.imef_menu_mode = !context.imef_menu_mode;
            log::debug!("menu mode for token {:?} is now {}", token, context.imef_menu_mode);
        }
    }
    pub(crate) fn raise_menu(&mut self,
        name: &str,
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<(), xous::Error> {
        log::debug!("looking for menu {}", name);
        if let Some(token) = self.find_app_token_by_name(name) {
            log::debug!("found menu token: {:?}", token);
            if let Some(context) = self.get_context_by_token(token) {
                log::debug!("found menu context");
                // don't allow raising of "apps" without authentication
                // but alerts can be raised without authentication
                if context.layout.behavior() == LayoutBehavior::Alert {
                    log::debug!("activating context");
                    return self.activate(gfx, canvases, token, false)
                } else {
                    return Err(xous::Error::AccessDenied)
                }
            }
        }
        Err(xous::Error::ProcessNotFound)
    }
}
