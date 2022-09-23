#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod mainmenu;
use keyboard::KeyMap;
use mainmenu::*;
mod appmenu;
use appmenu::*;
mod kbdmenu;
use kbdmenu::*;
mod app_autogen;
mod time;
mod ecup;
mod wifi;

use com::api::*;
use root_keys::api::{BackupOp, BackupKeyboardLayout};
use core::fmt::Write;
use num_traits::*;
use xous::{msg_scalar_unpack, send_message, Message, CID};
use graphics_server::*;
use graphics_server::api::GlyphStyle;
use locales::t;
use gam::{GamObjectList, GamObjectType};
use chrono::prelude::*;
use crossbeam::channel::{at, select, unbounded, Receiver, Sender};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg_attr(
    not(any(target_os = "none", target_os = "xous")),
    allow(unused_imports)
)]
use std::thread;

const SERVER_NAME_STATUS_GID: &str = "_Status bar GID receiver_";
const SERVER_NAME_STATUS: &str = "_Status_";
/// How long a backup header should persist before it is automatically deleted.
/// The interval is picked to be long enough for a user to have ample time to get the backup,
/// (even with multiple retries due to e.g. some hardware problem or logistical issue)
/// but not so long that we're likely to have expired compatibility revision data
/// in the header metadata. Initially, it's set at one day until it is automatically deleted.
const BACKUP_EXPIRATION_HOURS: i64 = 24;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum StatusOpcode {
    /// for passing battstats on to the main thread from the callback
    BattStats,
    /// indicates time for periodic update of the status bar
    Pump,
    /// Initiates a reboot
    Reboot,

    /// Raise the PDDB menu
    SubmenuPddb,
    /// Raise the App menu
    SubmenuApp,
    /// Raise the Keyboard layout menu
    SubmenuKbd,

    /// Raise the Shellchat app
    SwitchToShellchat,
    /// Switch to an app
    SwitchToApp,

    /// Set the keyboard map
    SetKeyboard,

    /// Prepare for a backup
    PrepareBackup,

    /// Tells keyboard watching thread that a new keypress happened.
    Keypress,
    /// Turns backlight off.
    TurnLightsOff,
    /// Turns backlight on.
    TurnLightsOn,
    /// Enables automatic backlight handling.
    EnableAutomaticBacklight,
    /// Disables automatic backlight handling.
    DisableAutomaticBacklight,

    /// Suspend handler from the main menu
    TrySuspend,
    /// Ship mode handler for the main menu
    BatteryDisconnect,
    /// for returning wifi stats
    WifiStats,

    /// Raise the wifi menu
    WifiMenu,
    Quit,
}

static mut CB_TO_MAIN_CONN: Option<CID> = None;
fn battstats_cb(stats: BattStats) {
    if let Some(cb_to_main_conn) = unsafe { CB_TO_MAIN_CONN } {
        let rawstats: [usize; 2] = stats.into();
        send_message(
            cb_to_main_conn,
            xous::Message::new_scalar(
                StatusOpcode::BattStats.to_usize().unwrap(),
                rawstats[0],
                rawstats[1],
                0,
                0,
            ),
        )
        .unwrap();
    }
}

pub fn pump_thread(conn: usize, pump_run: Arc<AtomicBool>) {
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    loop {
        if pump_run.load(Ordering::Relaxed) {
            match send_message(
                conn as u32,
                Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
            ) {
                Err(xous::Error::ServerNotFound) => break,
                Ok(xous::Result::Ok) => {}
                _ => panic!("unhandled error in status pump thread"),
            }
        }
        ticktimer.sleep_ms(1000).unwrap();
    }
}
fn main () -> ! {
    #[cfg(not(feature="ditherpunk"))]
    wrapped_main();

    #[cfg(feature="ditherpunk")]
    let stack_size = 1024 * 1024;
    #[cfg(feature="ditherpunk")]
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(wrapped_main)
        .unwrap()
        .join()
        .unwrap()
}
fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // this kicks off the thread that services the `libstd` calls for time-related things.
    // we want this started really early, because it sanity checks the RTC and a bunch of other stuff.
    time::start_time_server();

    // ------------------ acquire the status canvas GID
    let xns = xous_names::XousNames::new().unwrap();
    // 1 connection exactly -- from the GAM to set our canvas GID
    let status_gam_getter = xns
        .register_name(SERVER_NAME_STATUS_GID, Some(1))
        .expect("can't register server");
    let mut canvas_gid: [u32; 4] = [0; 4];
    // wait unil we're assigned a GID -- this is a one-time message from the GAM
    let msg = xous::receive_message(status_gam_getter).unwrap();
    log::trace!("GID assignment message: {:?}", msg);
    xous::msg_scalar_unpack!(msg, g0, g1, g2, g3, {
        canvas_gid[0] = g0 as u32;
        canvas_gid[1] = g1 as u32;
        canvas_gid[2] = g2 as u32;
        canvas_gid[3] = g3 as u32;
    });
    match xns.unregister_server(status_gam_getter) {
        Err(e) => {
            log::error!("couldn't unregister getter server: {:?}", e);
        }
        _ => {}
    }
    xous::destroy_server(status_gam_getter).unwrap();

    // ------------------ lay out our public API infrastructure
    // ok, now that we have a GID, we can continue on with our merry way
    let status_gid: Gid = Gid::new(canvas_gid);
     // allow only one connection, from keyboard to us.
    let status_sid = xns.register_name(SERVER_NAME_STATUS, Some(1)).unwrap();
    // create a connection for callback hooks
    let cb_cid = xous::connect(status_sid).unwrap();
    unsafe { CB_TO_MAIN_CONN = Some(cb_cid) };
    let pump_run = Arc::new(AtomicBool::new(false));
    let pump_conn = xous::connect(status_sid).unwrap();
    let _ = thread::spawn({
        let pump_run = pump_run.clone();
        move || {
            pump_thread(pump_conn as _, pump_run);
        }
    });
    // used to show notifications, e.g. can't sleep while power is engaged.
    let modals = modals::Modals::new(&xns).unwrap();

    // ------------------ start a 'gutter' thread to handle incoming events while we go through the boot/autoupdate process
    let gutter = thread::spawn({
        let gutter_sid = status_sid.clone();
        move || {
            loop {
                let msg = xous::receive_message(gutter_sid).unwrap();
                let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
                log::info!("Guttering {:?}", opcode);
                match opcode {
                    Some(StatusOpcode::Quit) => {
                        xous::return_scalar(msg.sender, 1).ok();
                        break;
                    }
                    _ => () // ignore everything else.
                }
            }
        }
    });

    // ------------------ render initial graphical display, so we don't seem broken on boot
    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let susres = susres::Susres::new_without_hook(&xns).unwrap();
    let mut netmgr = net::NetManager::new();

    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam
        .get_canvas_bounds(status_gid)
        .expect("|status: Couldn't get canvas size");
    // layout: 336 px wide
    // 0                   150 150 200
    // Feb 05 15:00 (00:06:23) xxxx     3.72V/-100mA/99%
    const CPU_BAR_WIDTH: i16 = 46;
    const CPU_BAR_OFFSET: i16 = 8;
    let time_rect = Rectangle::new_with_style(
        Point::new(0, 0),
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2 - 1 + CPU_BAR_OFFSET, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0)
    );
    let cpuload_rect = Rectangle::new_with_style(
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2 + CPU_BAR_OFFSET, 0),
        Point::new(screensize.x / 2 + CPU_BAR_WIDTH / 2 + CPU_BAR_OFFSET, screensize.y / 2 + 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Dark, 1),
    );
    let stats_rect = Rectangle::new_with_style(
        Point::new(screensize.x / 2 + CPU_BAR_WIDTH / 2 + 1 + CPU_BAR_OFFSET, 0),
        Point::new(screensize.x, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
    );

    log::debug!("|status: building textview objects");
    // build uptime text view: left half of status bar
    let mut uptime_tv = TextView::new(
        status_gid,
        TextBounds::GrowableFromTl(time_rect.tl(), time_rect.width() as _),
    );
    uptime_tv.untrusted = false;
    uptime_tv.style = GlyphStyle::Regular;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(3, 0);
    write!(uptime_tv, "{}", t!("secnote.startup", xous::LANG)).expect("|status: couldn't init uptime text");
    gam.post_textview(&mut uptime_tv)
        .expect("|status: can't draw battery stats");
    log::debug!("|status: screensize as reported: {:?}", screensize);
    log::debug!("|status: uptime initialized to '{:?}'", uptime_tv);

    // build battstats text view: right half of status bar
    let mut battstats_tv = TextView::new(
        status_gid,
        TextBounds::GrowableFromTr(stats_rect.tr(), stats_rect.width() as _),
    );
    battstats_tv.style = GlyphStyle::Regular;
    battstats_tv.draw_border = false;
    battstats_tv.margin = Point::new(0, 0);
    gam.post_textview(&mut battstats_tv)
        .expect("|status: can't draw battery stats");

    // initialize to some "sane" mid-point defaults, so we don't trigger errors later on before the first real battstat reading comes
    let mut stats = BattStats {
        voltage: 3700,
        soc: 50,
        current: 0,
        remaining_capacity: 650,
    };

    let llio = llio::Llio::new(&xns);
    let usb_hid = usb_device_xous::UsbHid::new();

    log::debug!("usb unlock notice...");
    let (dl, _) = usb_hid.debug_usb(None).unwrap();
    let mut debug_locked = dl;
    // build security status textview
    let mut security_tv = TextView::new(
        status_gid,
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, screensize.y / 2 + 1),
            Point::new(screensize.x, screensize.y),
        )),
    );
    security_tv.style = GlyphStyle::Regular;
    security_tv.draw_border = false;
    security_tv.margin = Point::new(0, 0);
    security_tv.token = gam.claim_token(gam::STATUS_BAR_NAME).expect("couldn't request token"); // this is a shared magic word to identify this process
    security_tv.clear_area = true;
    security_tv.invert = true;
    write!(&mut security_tv, "{}", t!("secnote.startup", xous::LANG)).unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.draw_line(status_gid, Line::new_with_style(
        Point::new(0, screensize.y), screensize,
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1))).unwrap();
    log::trace!("status redraw## initial");
    gam.redraw().unwrap(); // initial boot redraw

    // ------------------------ measure current security state and adjust messaging
    let sec_notes = Arc::new(Mutex::new(HashMap::new()));
    let mut last_sec_note_index = 0;
    let mut last_sec_note_size = 0;
    if !debug_locked {
        sec_notes.lock().unwrap().insert(
            "secnote.usb_unlock".to_string(),
            t!("secnote.usb_unlock", xous::LANG).to_string(),
        );
    }

    // --------------------------- spawn a time UX manager thread
    let time_sid = xous::create_server().unwrap();
    let time_cid = xous::connect(time_sid).unwrap();
    time::start_time_ux(time_sid);
    // this is used by the main loop to get the localtime to show on the status bar
    let mut localtime = llio::LocalTime::new();

    // ---------------------------- connect to the com & root keys server (prereq for menus)
    let keys = Arc::new(Mutex::new(
        root_keys::RootKeys::new(&xns, None)
            .expect("couldn't connect to root_keys to query initialization state"),
    ));
    let mut com = com::Com::new(&xns).expect("|status: can't connect to COM");

    // ---------------------------- build menus
    // used to hide time when the PDDB is not mounted
    let pddb_poller = pddb::PddbMountPoller::new();
    // these menus stake a claim on some security-sensitive connections; occupy them upstream of trying to do an update
    log::debug!("starting main menu thread");
    let main_menu_sid = xous::create_server().unwrap();
    let status_cid = xous::connect(status_sid).unwrap();
    let menu_manager = create_main_menu(keys.clone(), main_menu_sid, status_cid, &com, time_cid);
    create_app_menu(xous::connect(status_sid).unwrap());
    let kbd_mgr = xous::create_server().unwrap();
    let kbd_menumatic = create_kbd_menu(xous::connect(status_sid).unwrap(), kbd_mgr);
    let kbd = keyboard::Keyboard::new(&xns).unwrap();

    // ---------------------------- Automatic backlight-related variables.
    // must be upstream of the update check, because we need to occupy the keyboard
    // server slot to prevent e.g. a keyboard logger from taking our passwords!
    kbd.register_observer(
        SERVER_NAME_STATUS,
        StatusOpcode::Keypress.to_u32().unwrap() as usize,
    );

    let enabled = Arc::new(Mutex::new(false));
    let (tx, rx): (Sender<BacklightThreadOps>, Receiver<BacklightThreadOps>) = unbounded();

    let rx = Box::new(rx);

    let thread_already_running = Arc::new(Mutex::new(false));
    let thread_conn = xous::connect(status_sid).unwrap();

    // ------------------------ check firmware status and apply updates
    // all security sensitive servers must be occupied at this point in time.
    // in debug mode, anyone could, in theory, connect to and trigger an EC update, given this permissive policy.
    #[cfg(feature="dbg-ecupdate")]
    let ecup_sid = xns.register_name("__ECUP server__", None).unwrap(); // do not change name, it is referred to in shellchat
    #[cfg(not(feature="dbg-ecupdate"))]
    let ecup_sid = xous::create_server().unwrap(); // totally private in this mode
    let _ = thread::spawn({
        move || {
            ecup::ecupdate_thread(ecup_sid);
        }
    });
    let ecup_conn = xous::connect(ecup_sid).unwrap();
    // check & automatically apply any EC updates
    let mut ec_updated = false;
    let mut soc_updated = false;
    match send_message(ecup_conn,
        Message::new_blocking_scalar(ecup::UpdateOp::UpdateAuto.to_usize().unwrap(), 0, 0, 0, 0)
    ).expect("couldn't send auto update command") {
        xous::Result::Scalar1(r) => {
            match FromPrimitive::from_usize(r) {
                Some(ecup::UpdateResult::AutoDone) => {
                    // question: do we want to put something there that confirms that the reported EC firmware
                    // at this point matches the intended update? we /could/ do that, but if it fails then how?
                    ec_updated = true;
                    // restore interrupts and connection manager
                    llio.com_event_enable(true).ok();
                    netmgr.reset();
                    netmgr.connection_manager_run().ok();
                }
                Some(ecup::UpdateResult::NothingToDo) => log::info!("EC update check: nothing to do, firmware is up to date."),
                Some(ecup::UpdateResult::Abort) => {
                    modals.show_notification(t!("ecup.abort", xous::LANG), None).unwrap();
                }
                // note: invalid package triggers a pop-up in the update procedure, so we don't need to pop one up here.
                Some(ecup::UpdateResult::PackageInvalid) => log::error!("EC firmware package did not validate"),
                None => log::error!("invalid return code from EC update check"),
            }
        }
        _ => log::error!("Invalid return type from UpdateAuto"),
    }
    #[cfg(not(feature="dbg-ecupdate"))]
    { // if we're not debugging, quit the updater thread -- might as well free up the memory and connections if the thread is not callable
    // this frees up 28-40k runtime RAM + 1 connection in the status thread.
        send_message(ecup_conn,
            Message::new_blocking_scalar(ecup::UpdateOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't quit updater thread");
        unsafe{xous::disconnect(ecup_conn).ok()};
    }

    // check for backups after EC updates, but before we check for gateware updates
    let mut backup_time: Option<DateTime::<Utc>> = None;
    let mut restore_running = false;
    let restore_header = keys.lock().unwrap().get_restore_header();
    match restore_header {
        Ok(Some(header)) => {
            log::info!("Restore header op: {:?}", header.op);
            match header.op {
                BackupOp::Restore => {
                    // set the keyboard layout according to the restore record.
                    let map_deserialize: BackupKeyboardLayout = header.kbd_layout.into();
                    let map: KeyMap = map_deserialize.into();
                    log::info!("Keyboard layout set to {:?} by restore process.", map);
                    kbd.set_keymap(map).ok();

                    let backup_dna = u64::from_le_bytes(header.dna);
                    if backup_dna != llio.soc_dna().unwrap() {
                        log::info!("This will be a two-stage restore because this is to a new device.");
                        log::info!("backup_dna is 0x{:x}", backup_dna);
                        log::info!("reported dna is 0x{:x}", llio.soc_dna().unwrap());
                    }
                    keys.lock().unwrap().do_restore_backup_ux_flow();
                    // if the DNA matches the backup, the backup header is automatically erased.
                    restore_running = true;
                }
                BackupOp::RestoreDna => {
                    let pddb = pddb::Pddb::new();
                    let backup_dna = u64::from_le_bytes(header.dna);
                    match pddb.rekey_pddb(pddb::PddbRekeyOp::FromDnaFast(backup_dna)) {
                        Ok(_) => {
                            // once this step is done & successful, we have to erase the backup block to avoid re-doing this flow
                            keys.lock().unwrap().do_erase_backup();
                            log::info!("Rekey of PDDB to current device completed successfully")
                        },
                        Err(e) => {
                            modals.show_notification(&format!("{}{:?}", t!("rekey.fail", xous::LANG), e), None).ok();
                            log::error!("Backup was aborted. Reason: {:?}", e);
                        }
                    }
                }
                BackupOp::Backup => {
                    // once we have unlocked the PDDB and know our timezone, we'll compare the embedded timestamp to
                    // our current time, and delete the backup if it's too old.
                    backup_time = Some(
                        chrono::DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(header.timestamp as i64, 0),
                            chrono::offset::Utc
                        )
                    );
                }
                _ => log::warn!("backup record was found, but it has an improper operation field: {:?}", header.op),
            }
        }
        _ => {}, // no backup header found, continue with boot
    }

    // check for gateware updates
    let staged_sv = match keys.lock().unwrap().staged_semver() {
        Ok(sv) => {
            log::info!("Staged gateware version: {:?}", sv);
            Some(sv)
        },
        Err(xous::Error::InvalidString) => {log::info!("No staged gateware found; metadata is blank"); None},
        _ => {log::error!("Internal error reading staged gateware semantic version"); None},
    };
    if !keys.lock().unwrap().is_initialized().unwrap() {
        sec_notes.lock().unwrap().insert(
            "secnotes.no_keys".to_string(),
            t!("secnote.no_keys", xous::LANG).to_string(),
        );
        if !restore_running {
            if let Some(staged) = staged_sv {
                let soc = llio.soc_gitrev().expect("error querying SoC gitrev; this is fatal");
                if staged > soc { // we have a staged update, and no root keys. Just try to do the update.
                    if keys.lock().unwrap().try_nokey_soc_update() {
                        log::info!("No-touch SoC update successful");
                        soc_updated = true;
                    } else {
                        log::info!("No-touch SoC update was called, but then aborted");
                    }
                }
            }
            if !keys.lock().unwrap().is_dont_ask_set().unwrap_or(false) {
                keys.lock().unwrap().do_init_keys_ux_flow();
            }
        }
    } else if !restore_running {
        log::info!("checking gateware signature...");
        thread::spawn({
            let clone = Arc::clone(&sec_notes);
            let keys = Arc::clone(&keys);
            move || {
                if let Some(pass) = keys
                    .lock()
                    .unwrap()
                    .check_gateware_signature()
                    .expect("couldn't issue gateware check call")
                {
                    if !pass {
                        let mut sn = clone.lock().unwrap();
                        sn.insert(
                            "secnotes.gateware_fail".to_string(),
                            t!("secnote.gateware_fail", xous::LANG).to_string(),
                        );
                    }
                } else {
                    let mut sn = clone.lock().unwrap();
                    sn.insert(
                        "secnotes.state_fail".to_string(),
                        t!("secnote.state_fail", xous::LANG).to_string(),
                    );
                }
            }
        });
        if let Some(staged) = staged_sv {
            let soc = llio.soc_gitrev().expect("error querying SoC gitrev; this is fatal");
            if (staged > soc) && !soc_updated { // if the soc was updated, we should reboot before we try this
                if keys.lock().unwrap().prompt_for_update() {
                    // prompt to apply the update
                    modals.add_list_item(t!("rootkeys.gwup.yes", xous::LANG)).expect("couldn't build radio item list");
                    modals.add_list_item(t!("rootkeys.gwup.no", xous::LANG)).expect("couldn't build radio item list");
                    modals.add_list_item(t!("socup.ignore", xous::LANG)).expect("couldn't build radio item list");
                    match modals.get_radiobutton(t!("socup.candidate", xous::LANG)) {
                        Ok(response) => {
                            if response.as_str() == t!("rootkeys.gwup.yes", xous::LANG) {
                                keys.lock().unwrap().do_update_gw_ux_flow_blocking();
                                soc_updated = true;
                            } else if response.as_str() == t!("socup.ignore", xous::LANG) {
                                keys.lock().unwrap().set_update_prompt(false);
                            }
                        }
                        _ => (),
                    }
                }
            }
        }
    };
    sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: Shellchat").to_string()); // this is the default app on boot
    if ec_updated {
        netmgr.reset(); // have to do this to get the net manager stack into a known state after reset
    }
    if soc_updated {
        log::info!("Soc update was triggered, UX flow should be running now...");
    }
    // now that all the auto-update interaction is done, exit the gutter server. From
    // this point forward, messages will pile up in the status queue, until the main loop starts.
    send_message(cb_cid, Message::new_blocking_scalar(
        StatusOpcode::Quit.to_usize().unwrap(),
        0, 0, 0, 0,)
    ).expect("couldn't exit the gutter server");
    gutter.join().expect("status boot gutter server did not exit gracefully");

    // --------------------------- graphical loop timing
    let mut stats_phase: usize = 0;

    let charger_pump_interval = 180;
    let batt_interval;
    let secnotes_interval;
    if cfg!(feature = "braille") {
        // lower the status output rate for braille mode - it's invisible anyways, this is mainly for the debug configuration
        batt_interval = 60;
        secnotes_interval = 30;
    } else {
        batt_interval = 4;
        secnotes_interval = 4;
    }
    let mut battstats_phase = true;
    let mut secnotes_force_redraw = false;

    // --------------------------- sync to COM
    // the EC gets reset by the Net crate on boot to ensure that the state machines are synced up
    // this takes a few seconds, so we have a dead-wait here. This is a good spot for it because
    // the status bar reads "booting up..." during this period.
    log::debug!("syncing with COM");
    com.ping(0).unwrap(); // this will block until the COM is ready to take events
    com.hook_batt_stats(battstats_cb)
        .expect("|status: couldn't hook callback for events from COM");
    // prime the loop
    com.req_batt_stats()
        .expect("Can't get battery stats from COM");

    // ---------------------- final cleanup before entering main loop
    log::debug!("subscribe to wifi updates");
    netmgr.wifi_state_subscribe(cb_cid, StatusOpcode::WifiStats.to_u32().unwrap()).unwrap();
    let mut wifi_status: WlanStatus = WlanStatus::from_ipc(WlanStatusIpc::default());

    #[cfg(feature="tts")]
    thread::spawn({
        move || {
            // indicator of boot-up for blind users
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            tt.sleep_ms(1500).unwrap();
            let xns = xous_names::XousNames::new().unwrap();
            let llio = llio::Llio::new(&xns);
            llio.vibe(llio::VibePattern::Double).unwrap();
        }
    });
    log::info!("|status: starting main loop"); // don't change this -- factory test looks for this exact string

    // add a security note if we're booting with a "zero key"
    match keys.lock().unwrap().is_zero_key().expect("couldn't query zero key status") {
        Some(q) => match q {
            true => {
                sec_notes.lock().unwrap().insert(
                    "secnote.zero_key".to_string(),
                    t!("secnote.zero_key", xous::LANG).to_string(),
                );
            },
            false => {},
        }
        None => {
            {} // could be bbram, could be an error. could be keys are secured and disabled for readout. could be disabled-readout zero keys!
        }
    }
    #[cfg(any(target_os = "none", target_os = "xous"))]
    llio.clear_wakeup_alarm().unwrap(); // this is here to clear any wake-up alarms that were set by a prior coldboot command

    // spawn a thread to auto-mount the PDDB
    let _ = thread::spawn({
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            tt.sleep_ms(2000).unwrap(); // a brief pause, to allow the other startup bits to finish running
            pddb::Pddb::new().try_mount();
        }
    });

    wifi::start_background_thread();

    pump_run.store(true, Ordering::Relaxed); // start status thread updating
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(StatusOpcode::EnableAutomaticBacklight) => {
                *enabled.lock().unwrap() = true;

                // second: delete the first three elements off the menu
                menu_manager.delete_item(t!("mainmenu.backlighton", xous::LANG));
                menu_manager.delete_item(t!("mainmenu.backlightoff", xous::LANG));
                menu_manager.delete_item(t!("mainmenu.autobacklighton", xous::LANG));

                // third: add a "disable automatic backlight element"
                menu_manager.insert_item(gam::MenuItem {
                    name: xous_ipc::String::from_str(t!("mainmenu.autobacklightoff", xous::LANG)),
                    action_conn: Some(status_cid),
                    action_opcode: StatusOpcode::DisableAutomaticBacklight.to_u32().unwrap(),
                    action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                    close_on_select: true,
                }, 0);
            }
            Some(StatusOpcode::DisableAutomaticBacklight) => {
                *enabled.lock().unwrap() = false;
                tx.send(BacklightThreadOps::Stop).unwrap();

                // second: delete the first element off the menu.
                menu_manager.delete_item(t!("mainmenu.autobacklightoff", xous::LANG));

                // third: construct an array of the new elements to add to the menu.
                let new_elems = [
                    gam::MenuItem {
                        name: xous_ipc::String::from_str(t!("mainmenu.backlighton", xous::LANG)),
                        action_conn: Some(com.conn()),
                        action_opcode: com.getop_backlight(),
                        action_payload: gam::MenuPayload::Scalar([191 >> 3, 191 >> 3, 0, 0]),
                        close_on_select: true,
                    },
                    gam::MenuItem {
                        name: xous_ipc::String::from_str(t!("mainmenu.backlightoff", xous::LANG)),
                        action_conn: Some(com.conn()),
                        action_opcode: com.getop_backlight(),
                        action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    gam::MenuItem {
                        name: xous_ipc::String::from_str(t!("mainmenu.autobacklighton", xous::LANG)),
                        action_conn: Some(status_cid),
                        action_opcode: StatusOpcode::EnableAutomaticBacklight.to_u32().unwrap(),
                        action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    }
                ];

                new_elems.iter().enumerate().for_each(|(index, element)| {let _ = menu_manager.insert_item(*element, index);});
            }
            Some(StatusOpcode::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                stats = [lo, hi].into();
                battstats_tv.clear_str();
                // have to clear the entire rectangle area, because the SSID has a variable width and can be much wider or shorter than battstats
                gam.draw_rectangle(status_gid, stats_rect).ok();

                // 0xdddd and 0xffff are what are returned when the EC is too busy to respond/hung, or in reset, respectively
                if stats.current == -8739 /* 0xdddd */
                || stats.voltage == 0xdddd || stats.voltage == 0xffff
                || stats.soc == 0xdd || stats.soc == 0xff {
                    write!(&mut battstats_tv, "{}", t!("stats.measuring", xous::LANG)).unwrap();
                } else {
                    // toggle between two views of the data every time we have a status update
                    let mut wattage_mw = (stats.current as i32 * stats.voltage as i32) / 1000i32;
                    let sign = if wattage_mw > 5 {
                        '\u{2b06}' // up arrow
                    } else if wattage_mw < -5 {
                        '\u{2b07}' // down arrow
                    } else {
                        '\u{1f50c}' // plugged in icon (e.g., fully charged, running on wall power now)
                    };
                    wattage_mw = wattage_mw.abs();
                    if battstats_phase {
                        write!(&mut battstats_tv, "{}.{:02}W{}{}.{:02}V {}%",
                        wattage_mw / 1000, wattage_mw % 1000,
                        sign,
                        stats.voltage as u32 / 1000, (stats.voltage as u32 % 1000) / 10, // 2 decimal places
                        stats.soc).unwrap();
                    } else {
                        if let Some(ssid) = wifi_status.ssid {
                            write!(
                                &mut battstats_tv,
                                "{} -{}dBm",
                                ssid.name.as_str().unwrap_or("UTF-8 Erorr"),
                                ssid.rssi,
                            ).unwrap();
                        } else {
                            if wifi_status.link_state == com_rs_ref::LinkState::ResetHold {
                                write!(
                                    &mut battstats_tv,
                                    "{}",
                                    t!("stats.wifi_off", xous::LANG)
                                ).unwrap();
                            } else {
                                write!(
                                    &mut battstats_tv,
                                    "{}",
                                    t!("stats.disconnected", xous::LANG)
                                ).unwrap();
                            }
                        }
                    }
                }
                gam.post_textview(&mut battstats_tv)
                    .expect("|status: can't draw battery stats");
                if let Some(bounds) = battstats_tv.bounds_computed {
                    if bounds.height() as i16 > screensize.y / 2 + 1 {
                        // the clipping rectangle limits the bounds to the overall height of the status area, so
                        // the overlap between status and secnotes must be managed within this server
                        log::info!("Status text overstepped its intended bound. Forcing secnotes redraw.");
                        secnotes_force_redraw = true;
                    }
                }
                battstats_phase = !battstats_phase;
            }),
            Some(StatusOpcode::WifiStats) => {
                let buffer = unsafe {
                    xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap())
                };
                wifi_status = WlanStatus::from_ipc(buffer.to_original::<com::WlanStatusIpc, _>().unwrap());
            },
            Some(StatusOpcode::WifiMenu) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::WIFI_MENU_NAME).unwrap();
            },
            Some(StatusOpcode::Pump) => {
                let elapsed_time = ticktimer.elapsed_ms();
                { // update the CPU load bar
                    let mut draw_list = GamObjectList::new(status_gid);
                    draw_list.push(GamObjectType::Rect(cpuload_rect)).unwrap();
                    let (latest_activity, period) = llio
                        .activity_instantaneous()
                        .expect("couldn't get CPU activity");
                    let activity_to_width = if period == 0 {
                        cpuload_rect.width() as i16 - 4
                    } else {
                        (((latest_activity as u64) * 1000u64 * (cpuload_rect.width() as u64 - 4)) / (period as u64 * 1000u64)) as i16
                    };
                    draw_list.push(GamObjectType::Rect(
                        Rectangle::new_coords_with_style(
                            cpuload_rect.tl().x + 2,
                            cpuload_rect.tl().y + 2,
                            cpuload_rect.tl().x + 2 + (activity_to_width).min(cpuload_rect.width() as i16 - 4),
                            cpuload_rect.br().y - 2,
                            DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 0))
                    )).unwrap();
                    gam.draw_list(draw_list).expect("couldn't draw object list");
                }

                // update the security status, if any
                let (is_locked, force_update) = usb_hid.debug_usb(None).unwrap();
                if (debug_locked != is_locked)
                    || force_update || secnotes_force_redraw
                    || sec_notes.lock().unwrap().len() != last_sec_note_size
                    || /*(sec_notes.lock().unwrap().len() > 1) // force the redraw periodically to clean up any tb overflow from uptime
                        &&*/ ((stats_phase % secnotes_interval) == 0)
                {
                    log::debug!("updating lock state text");
                    if debug_locked != is_locked {
                        if is_locked {
                            sec_notes
                                .lock()
                                .unwrap()
                                .remove(&"secnote.usb_unlock".to_string()); // this is the key, not the value to remove
                        } else {
                            sec_notes.lock().unwrap().insert(
                                "secnotes.usb_unlock".to_string(),
                                t!("secnote.usb_unlock", xous::LANG).to_string(),
                            );
                        }
                        debug_locked = is_locked;
                    }

                    if sec_notes.lock().unwrap().len() != last_sec_note_size {
                        last_sec_note_size = sec_notes.lock().unwrap().len();
                        if last_sec_note_size > 0 {
                            last_sec_note_index = last_sec_note_size - 1;
                        }
                    }

                    security_tv.clear_str();
                    if last_sec_note_size > 0 {
                        for (index, v) in sec_notes.lock().unwrap().values().enumerate() {
                            if index == last_sec_note_index {
                                write!(&mut security_tv, "{}", v.as_str()).unwrap();
                                last_sec_note_index =
                                    (last_sec_note_index + 1) % last_sec_note_size;
                                break;
                            }
                        }
                    } else {
                        write!(&mut security_tv, "{}", t!("secnote.allclear", xous::LANG)).unwrap();
                    }

                    secnotes_force_redraw = false;
                    gam.post_textview(&mut security_tv).unwrap();
                    gam.draw_line(status_gid, Line::new_with_style(
                        Point::new(0, screensize.y), screensize,
                        DrawStyle::new(PixelColor::Light, PixelColor::Light, 1))).unwrap();
                }
                if (stats_phase % batt_interval) == (batt_interval - 1) {
                    com.req_batt_stats()
                        .expect("Can't get battery stats from COM");
                }
                if (stats_phase % charger_pump_interval) == 1 {
                    // stagger periodic tasks
                    // confirm that the charger is in the right state.
                    if stats.soc < 95 || stats.remaining_capacity < 1000 {
                        // only request if we aren't fully charged, either by SOC or capacity metrics
                        if (llio.adc_vbus().unwrap() as u32) * 503 > 445_000 { // 0.005033 * 100_000 against 4.45V * 100_000
                            // 4.45V is our threshold for deciding if a cable is present
                            // charging cable is present
                            if !com.is_charging().expect("couldn't check charging state") {
                                // not charging, but cable is present
                                log::debug!("Charger present, but not currently charging. Automatically requesting charge start.");
                                com.request_charging()
                                    .expect("couldn't send charge request");
                            }
                        }
                    }
                }
                { // update the time field
                    // have to clear the entire rectangle area, because the text has a variable width and dirty text will remain if the text is shortened
                    gam.draw_rectangle(status_gid, time_rect).ok();
                    uptime_tv.clear_str();
                    if let Some(timestamp) = localtime.get_local_time_ms() {
                        // we "say" UTC but actually local time is in whatever the local time is
                        let dt = chrono::DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(timestamp as i64 / 1000, 0),
                            chrono::offset::Utc
                        );
                        let timestr = dt.format("%H:%M %m/%d").to_string();
                        // TODO: convert dt to an actual local time using the chrono library
                        write!(
                            &mut uptime_tv,
                            "{}",
                            timestr
                        )
                        .unwrap();
                        if let Some(bt) = backup_time {
                            let since_backup = dt.signed_duration_since(bt);
                            if since_backup.num_hours().abs() > BACKUP_EXPIRATION_HOURS {
                                keys.lock().unwrap().do_erase_backup();
                                backup_time = None;
                            }
                        }
                    } else {
                        if pddb_poller.is_mounted_nonblocking() {
                            write!(
                                &mut uptime_tv,
                                "{}",
                                t!("stats.set_time", xous::LANG)
                            ).unwrap();
                        } else {
                            write!(
                                &mut uptime_tv,
                                "{}",
                                t!("stats.mount_pddb", xous::LANG)
                            ).unwrap();
                        }
                    }
                    // use ticktimer, not stats_phase, because stats_phase encodes some phase drift due to task-switching overhead
                    write!(
                        &mut uptime_tv,
                        " {}{}:{:02}:{:02}",
                        t!("stats.uptime", xous::LANG),
                        (elapsed_time / 3_600_000),
                        (elapsed_time / 60_000) % 60,
                        (elapsed_time / 1000) % 60,
                    )
                    .expect("|status: can't write string");
                    gam.post_textview(&mut uptime_tv)
                        .expect("|status: can't draw uptime");
                    if let Some(bounds) = uptime_tv.bounds_computed {
                        if bounds.height() as i16 > screensize.y / 2 + 1 {
                            // the clipping rectangle limits the bounds to the overall height of the status area, so
                            // the overlap between status and secnotes must be managed within this server
                            log::info!("Status text overstepped its intended bound. Forcing secnotes redraw.");
                            secnotes_force_redraw = true;
                        }
                    }
                }
                log::trace!("status redraw## update");
                gam.redraw().expect("|status: couldn't redraw");

                stats_phase = stats_phase.wrapping_add(1);
            }
            Some(StatusOpcode::Reboot) => {
                if ((llio.adc_vbus().unwrap() as u32) * 503) > 150_000 { // 0.005033 * 100_000 against 1.5V * 100_000
                    // power plugged in, do a reboot using a warm boot method
                    susres.reboot(true).expect("couldn't issue reboot command");
                } else {
                    // ensure the self-boosting facility is turned off, this interferes with a cold boot
                    com.set_boost(false).ok();
                    llio.boost_on(false).ok();
                    // do a full cold-boot if the power is cut. This will force a re-load of the SoC contents.
                    gam.shipmode_blank_request().ok();
                    ticktimer.sleep_ms(500).ok(); // screen redraw time after the blank request
                    llio.set_wakeup_alarm(4).expect("couldn't set wakeup alarm");
                    llio.allow_ec_snoop(true).ok();
                    llio.allow_power_off(true).ok();
                    com.power_off_soc().ok();
                    ticktimer.sleep_ms(4000).ok();
                    panic!("system did not reboot");
                }
            }
            Some(StatusOpcode::SubmenuPddb) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::PDDB_MENU_NAME).expect("couldn't raise PDDB submenu");
            },
            Some(StatusOpcode::SubmenuApp) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::APP_MENU_NAME).expect("couldn't raise App submenu");
            },
            Some(StatusOpcode::SubmenuKbd) => {
                log::debug!("getting keyboard map");
                let map = kbd.get_keymap().expect("couldn't get key mapping");
                log::info!("setting keymap index to {:?}", map);
                kbd_menumatic.set_index(map.into());
                log::debug!("raising keyboard menu");
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::KBD_MENU_NAME).expect("couldn't raise keyboard layout submenu");
            },
            Some(StatusOpcode::SetKeyboard) => msg_scalar_unpack!(msg, code, _, _, _, {
                let map = keyboard::KeyMap::from(code);
                kbd.set_keymap(map).expect("couldn't set keyboard mapping");
            }),
            Some(StatusOpcode::SwitchToShellchat) => {
                ticktimer.sleep_ms(100).ok();
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: Shellchat").to_string());
                gam.switch_to_app(gam::APP_NAME_SHELLCHAT, security_tv.token.unwrap()).expect("couldn't raise shellchat");
                secnotes_force_redraw = true;
                send_message(
                    cb_cid,
                    Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                ).expect("couldn't trigger status update");
            },
            Some(StatusOpcode::SwitchToApp) => msg_scalar_unpack!(msg, index, _, _, _, {
                ticktimer.sleep_ms(100).ok();
                let app_name = app_autogen::app_index_to_name(index).expect("app index not found");
                app_autogen::app_dispatch(&gam, security_tv.token.unwrap(), index).expect("cannot switch to app");
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes.lock().unwrap().insert("current_app".to_string(), format!("Running: {}", app_name).to_string());
                secnotes_force_redraw = true;
                send_message(
                    cb_cid,
                    Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                ).expect("couldn't trigger status update");
            }),
            Some(StatusOpcode::TrySuspend) => {
                if ((llio.adc_vbus().unwrap() as u32) * 503) > 150_000 {
                    modals.show_notification(t!("mainmenu.cant_sleep", xous::LANG), None).expect("couldn't notify that power is plugged in");
                } else {
                    // log::set_max_level(log::LevelFilter::Debug);
                    match susres.initiate_suspend() {
                        Ok(_) => {},
                        Err(xous::Error::Timeout) => {
                            modals.show_notification(t!("suspend.fail", xous::LANG), None).unwrap();
                        }
                        Err(_e) => {
                            panic!("Unhandled error on suspend request");
                        }
                    }
                }
            },
            Some(StatusOpcode::BatteryDisconnect) => {
                if ((llio.adc_vbus().unwrap() as u32) * 503) > 150_000 {
                    modals.show_notification(t!("mainmenu.cant_sleep", xous::LANG), None).expect("couldn't notify that power is plugged in");
                } else {
                    gam.shipmode_blank_request().ok();
                    ticktimer.sleep_ms(500).unwrap();
                    llio.allow_ec_snoop(true).unwrap();
                    llio.allow_power_off(true).unwrap();
                    com.ship_mode().unwrap();
                    com.power_off_soc().unwrap();
                }
            },

            Some(StatusOpcode::Keypress) => {
                if !*enabled.lock().unwrap() {
                    log::trace!("ignoring keypress, automatic backlight is disabled");
                    continue
                }
                let mut run_lock = thread_already_running.lock().unwrap();
                match *run_lock {
                    true => {
                        log::trace!("renewing backlight timer");
                        tx.send(BacklightThreadOps::Renew).unwrap();
                        continue
                    },
                    false => {
                        *run_lock = true;
                        com.set_backlight(255, 128).expect("cannot set backlight on");
                        std::thread::spawn({
                            let rx = rx.clone();
                            move || turn_lights_on(rx, thread_conn)
                        });
                    },
                }
            }
            Some(StatusOpcode::TurnLightsOn) => {
                log::trace!("turning lights on");
                com.set_backlight(255, 128).expect("cannot set backlight on");
            },
            Some(StatusOpcode::TurnLightsOff) => {
                log::trace!("turning lights off");
                let mut run_lock = thread_already_running.lock().unwrap();
                *run_lock = false;
                com.set_backlight(0, 0).expect("cannot set backlight off");
            },
            Some(StatusOpcode::PrepareBackup) => {
                // sync the PDDB to disk prior to making backups
                let pddb = pddb::Pddb::new();
                pddb.sync().expect("couldn't synchronize PDDB to disk");
                let mut metadata = root_keys::api::BackupHeader::default();
                // note: default() should set the language correctly by default since it's a systemwide constant
                metadata.timestamp = localtime.get_local_time_ms().unwrap_or(0);
                metadata.xous_ver = ticktimer.get_version_semver().into();
                metadata.soc_ver = llio.soc_gitrev().unwrap().into();
                metadata.wf200_ver = com.get_wf200_fw_rev().unwrap().into();
                metadata.ec_ver = com.get_ec_sw_tag().unwrap().into();
                metadata.op = BackupOp::Backup;
                metadata.dna = llio.soc_dna().unwrap().to_le_bytes();
                let map = kbd.get_keymap().expect("couldn't get key mapping");
                let map_serialize: BackupKeyboardLayout = map.into();
                metadata.kbd_layout = map_serialize.into();
                keys.lock().unwrap().do_create_backup_ux_flow(metadata);
            }
            Some(StatusOpcode::Quit) => {
                xous::return_scalar(msg.sender, 1).ok();
                break;
            }
            None => {
                log::error!("|status: received unknown Opcode");
            }
        }
    }
    log::trace!("status thread exit, destroying servers");
    unsafe {
        if let Some(cb) = CB_TO_MAIN_CONN {
            xous::disconnect(cb).unwrap();
        }
    }
    unsafe {
        xous::disconnect(pump_conn).unwrap();
    }
    xns.unregister_server(status_sid).unwrap();
    xous::destroy_server(status_sid).unwrap();
    log::trace!("status thread quitting");
    xous::terminate_process(0)
}

enum BacklightThreadOps {
    /// Renew renews the backlight timer for another instance of standard_duration.
    Renew,

    /// Stop stops the background backlight thread.
    Stop,
}

fn turn_lights_on(rx: Box<Receiver<BacklightThreadOps>>, cid: xous::CID) {
    let standard_duration = std::time::Duration::from_secs(10);

    let mut timeout = std::time::Instant::now() + standard_duration;

    let mut total_waited = 0;

    loop {
        select! {
            recv(rx) -> op => {
                match op.unwrap() {
                    BacklightThreadOps::Renew => {
                        timeout = std::time::Instant::now() + standard_duration;
                        total_waited += 1;
                    },
                    BacklightThreadOps::Stop => {
                        log::trace!("received Stop op, killing background backlight thread");
                        xous::send_message(cid, xous::Message::new_scalar(StatusOpcode::TurnLightsOff.to_usize().unwrap(), 0,0,0,0)).unwrap();
                        return;
                    },
                }
            },
            recv(at(timeout)) -> _ => {
                log::trace!("timeout finished, total re-waited {}, returning!", total_waited);
                xous::send_message(cid, xous::Message::new_scalar(StatusOpcode::TurnLightsOff.to_usize().unwrap(), 0,0,0,0)).unwrap();
                break;
            }
        };
    }
}
