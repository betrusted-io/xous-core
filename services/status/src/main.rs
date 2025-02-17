#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod mainmenu;
use keyboard::KeyMap;
use mainmenu::*;
mod appmenu;
use appmenu::*;
mod app_autogen;
mod ecup;
mod preferences;
mod wifi;

use core::fmt::Write;
use core::sync::atomic::AtomicU32;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg_attr(not(target_os = "xous"), allow(unused_imports))]
use std::thread;

use blitstr2::GlyphStyle;
use chrono::prelude::*;
use com::api::*;
use crossbeam::channel::{Receiver, Sender, at, select, unbounded};
use gam::{GamObjectList, GamObjectType};
use graphics_server::*;
use locales::t;
use num_traits::*;
use root_keys::api::{BackupKeyboardLayout, BackupOp};
use xous::{CID, Message, msg_scalar_unpack, send_message};

use crate::preferences::{PrefsMenuUpdateOp, percentage_to_db};

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

    /// Raise the Shellchat app
    SwitchToShellchat,
    /// Switch to an app
    SwitchToApp,

    /// Prepare for a backup
    PrepareBackup,
    PrepareBackupConfirmed,
    PrepareBackupPhase2,

    /// Burn a backup key
    #[cfg(feature = "efuse")]
    BurnBackupKey,

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
    /// Reloads preference variables from PDDB. Called by preferences manager when a variable is updated.
    /// The usage may not be consistent, because this was patched in after the initial architecture was set
    /// up.
    ReloadPrefs,

    /// Suspend handler from the main menu
    TrySuspend,
    /// Ship mode handler for the main menu
    BatteryDisconnect,
    /// for returning wifi stats
    WifiStats,

    /// Forces EC update
    ForceEcUpdate,

    /// Raise the preferences menu
    Preferences,
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

pub fn pump_thread(
    conn: usize,
    pump_run: Arc<AtomicBool>,
    last_key_hit_secs: Arc<AtomicU32>,
    autosleep_duration_mins: Arc<AtomicU32>,
    reboot_on_autosleep: Arc<AtomicBool>,
) {
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();
    let llio = llio::Llio::new(&xns);
    let mut last_power_state = llio.is_plugged_in();
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

        let cur_power_state = llio.is_plugged_in();
        // note that this will miss fast plug/unplug events of < 1 second
        if last_power_state != cur_power_state {
            log::debug!("power state change detected, resetting timer");
            // power state changed. Consider this a "key press" for the purposes of auto-events.
            last_key_hit_secs.store((ticktimer.elapsed_ms() / 1000) as u32, Ordering::SeqCst);
            last_power_state = cur_power_state;
        }

        let asdm = autosleep_duration_mins.load(Ordering::SeqCst);
        if asdm != 0 {
            let last_key_hit_duration_mins =
                ((ticktimer.elapsed_ms() / 1000) as u32 - last_key_hit_secs.load(Ordering::SeqCst)) / 60;
            if last_key_hit_duration_mins >= asdm {
                log::debug!("autosleep duration hit, trying to sleep");
                if cur_power_state == false {
                    // is_plugged_in() is false
                    if reboot_on_autosleep.load(Ordering::SeqCst) {
                        log::info!("Autolocking...");
                        send_message(
                            conn as u32,
                            Message::new_scalar(StatusOpcode::Reboot.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .ok();
                    } else {
                        log::info!("Autosleeping...");
                        send_message(
                            conn as u32,
                            Message::new_scalar(StatusOpcode::TrySuspend.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .ok();
                    }
                } else {
                    log::debug!("can't sleep, plugged in!");
                }
            }
        }

        // TODO: autounmount
    }
}
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

    // ------------------ acquire the status canvas GID
    let xns = xous_names::XousNames::new().unwrap();
    let early_settings = early_settings::EarlySettings::new(&xns).unwrap();
    // 1 connection exactly -- from the GAM to set our canvas GID
    let status_gam_getter =
        xns.register_name(SERVER_NAME_STATUS_GID, Some(1)).expect("can't register server");
    let mut canvas_gid: [u32; 4] = [0; 4];
    // wait until we're assigned a GID -- this is a one-time message from the GAM
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
    // Expected connections:
    //   - from keyboard
    //   - from USB HID
    let status_sid = xns.register_name(SERVER_NAME_STATUS, Some(2)).unwrap();
    // create a connection for callback hooks
    let cb_cid = xous::connect(status_sid).unwrap();
    unsafe { CB_TO_MAIN_CONN = Some(cb_cid) };
    let pump_run = Arc::new(AtomicBool::new(false));
    // allocate shared variables for automatic timers that get polled in the pump thread
    let last_key_hit_secs = Arc::new(AtomicU32::new(0)); // rolls over in 126 years. Can't AtomicU64 on a 32-bit platform.
    let autosleep_duration_mins = Arc::new(AtomicU32::new(0));
    let reboot_on_autosleep = Arc::new(AtomicBool::new(false));
    let autobacklight_duration_secs = Arc::new(AtomicU32::new(0));
    let pump_conn = xous::connect(status_sid).unwrap();
    let _ = thread::spawn({
        let pump_run = pump_run.clone();
        let last_key_hit_secs = last_key_hit_secs.clone();
        let autosleep_duration_mins = autosleep_duration_mins.clone();
        let reboot_on_autosleep = reboot_on_autosleep.clone();
        move || {
            pump_thread(
                pump_conn as _,
                pump_run,
                last_key_hit_secs,
                autosleep_duration_mins,
                reboot_on_autosleep,
            );
        }
    });
    // used to show notifications, e.g. can't sleep while power is engaged.
    let modals = modals::Modals::new(&xns).unwrap();

    // ------------------ start a 'gutter' thread to handle incoming events while we go through the
    // boot/autoupdate process
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
                    _ => (), // ignore everything else.
                }
            }
        }
    });

    // ------------------ render initial graphical display, so we don't seem broken on boot
    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let susres = susres::Susres::new_without_hook(&xns).unwrap();
    let mut netmgr = net::NetManager::new();
    #[cfg(not(feature = "no-codec"))]
    let mut codec = codec::Codec::new(&xns).unwrap();

    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam.get_canvas_bounds(status_gid).expect("|status: Couldn't get canvas size");
    // layout: 336 px wide
    // 0                   150 150 200
    // Feb 05 15:00 (00:06:23) xxxx     3.72V/-100mA/99%
    const CPU_BAR_WIDTH: i16 = 46;
    const CPU_BAR_OFFSET: i16 = 8;
    let time_rect = Rectangle::new_with_style(
        Point::new(0, 0),
        Point::new(screensize.x / 2 - CPU_BAR_WIDTH / 2 - 1 + CPU_BAR_OFFSET, screensize.y / 2 - 1),
        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
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
    let mut uptime_tv =
        TextView::new(status_gid, TextBounds::GrowableFromTl(time_rect.tl(), time_rect.width() as _));
    uptime_tv.untrusted = false;
    uptime_tv.style = GlyphStyle::Regular;
    uptime_tv.draw_border = false;
    uptime_tv.margin = Point::new(3, 0);
    write!(uptime_tv, "{}", t!("secnote.startup", locales::LANG))
        .expect("|status: couldn't init uptime text");
    gam.post_textview(&mut uptime_tv).expect("|status: can't draw battery stats");
    log::debug!("|status: screensize as reported: {:?}", screensize);
    log::debug!("|status: uptime initialized to '{:?}'", uptime_tv);

    // initialize to some "sane" mid-point defaults, so we don't trigger errors later on before the first real
    // battstat reading comes
    let mut stats = BattStats { voltage: 3700, soc: 50, current: 0, remaining_capacity: 650 };

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
    write!(&mut security_tv, "{}", t!("secnote.startup", locales::LANG)).unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.draw_line(
        status_gid,
        Line::new_with_style(
            Point::new(0, screensize.y),
            screensize,
            DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
        ),
    )
    .unwrap();
    log::trace!("status redraw## initial");
    gam.redraw().unwrap(); // initial boot redraw

    // ------------------------ measure current security state and adjust messaging
    let sec_notes = Arc::new(Mutex::new(HashMap::new()));
    let mut last_sec_note_index = 0;
    let mut last_sec_note_size = 0;
    if !debug_locked {
        sec_notes
            .lock()
            .unwrap()
            .insert("secnote.usb_unlock".to_string(), t!("secnote.usb_unlock", locales::LANG).to_string());
    }

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
    // these menus stake a claim on some security-sensitive connections; occupy them upstream of trying to do
    // an update
    log::debug!("starting main menu thread");
    let main_menu_sid = xous::create_server().unwrap();
    let status_cid = xous::connect(status_sid).unwrap();
    let menu_manager = create_main_menu(keys.clone(), main_menu_sid, status_cid, &com);
    create_app_menu(xous::connect(status_sid).unwrap());
    let kbd = Arc::new(Mutex::new(keyboard::Keyboard::new(&xns).unwrap()));

    // ---------------------------- Background processes that claim contexts
    // must be upstream of the update check, because we need to occupy the keyboard
    // server slot to prevent e.g. a keyboard logger from taking our passwords!
    kbd.lock()
        .unwrap()
        .register_observer(SERVER_NAME_STATUS, StatusOpcode::Keypress.to_u32().unwrap() as usize);
    // register the USB U2F event listener - point to the same handler as key press since our intention is to
    // just toggle the backlight
    usb_hid.register_u2f_observer(SERVER_NAME_STATUS, StatusOpcode::Keypress.to_u32().unwrap() as usize);

    let autobacklight_enabled = Arc::new(Mutex::new(true));
    let (tx, rx): (Sender<BacklightThreadOps>, Receiver<BacklightThreadOps>) = unbounded();

    let rx = Box::new(rx);

    let autobacklight_thread_already_running = Arc::new(Mutex::new(false));
    let thread_conn = xous::connect(status_sid).unwrap();

    let prefs_sid = xous::create_server().unwrap();
    let prefs_cid = xous::connect(prefs_sid).unwrap();
    preferences::start_background_thread(prefs_sid, status_cid);

    // load system preferences
    let prefs = Arc::new(Mutex::new(userprefs::Manager::new()));
    let prefs_thread_clone = prefs.clone();

    // ------------------------ check firmware status and apply updates
    // all security sensitive servers must be occupied at this point in time.
    // in debug mode, anyone could, in theory, connect to and trigger an EC update, given this permissive
    // policy.
    #[cfg(feature = "dbg-ecupdate")]
    let ecup_sid = xns.register_name("__ECUP server__", None).unwrap(); // do not change name, it is referred to in shellchat
    #[cfg(not(feature = "dbg-ecupdate"))]
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
    match send_message(
        ecup_conn,
        Message::new_blocking_scalar(ecup::UpdateOp::UpdateAuto.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .expect("couldn't send auto update command")
    {
        xous::Result::Scalar1(r) => {
            match FromPrimitive::from_usize(r) {
                Some(ecup::UpdateResult::AutoDone) => {
                    // question: do we want to put something there that confirms that the reported EC firmware
                    // at this point matches the intended update? we /could/ do that, but if it fails then
                    // how?
                    ec_updated = true;
                    // restore interrupts and connection manager
                    llio.com_event_enable(true).ok();
                    netmgr.reset();

                    // TODO(gsora): I'm commenting this out because user preferences might say otherwise.
                    //netmgr.connection_manager_run().ok();
                }
                Some(ecup::UpdateResult::NothingToDo) => {
                    log::info!("EC update check: nothing to do, firmware is up to date.")
                }
                Some(ecup::UpdateResult::Abort) => {
                    modals.show_notification(t!("ecup.abort", locales::LANG), None).unwrap();
                }
                // note: invalid package triggers a pop-up in the update procedure, so we don't need to pop
                // one up here.
                Some(ecup::UpdateResult::PackageInvalid) => {
                    log::error!("EC firmware package did not validate")
                }
                None => log::error!("invalid return code from EC update check"),
            }
        }
        _ => log::error!("Invalid return type from UpdateAuto"),
    }
    /* // leave this running, so we can force updates...costs some extra RAM but seems we need it as a matter of customer service
    #[cfg(not(feature="dbg-ecupdate"))]
    { // if we're not debugging, quit the updater thread -- might as well free up the memory and connections if the thread is not callable
    // this frees up 28-40k runtime RAM + 1 connection in the status thread.
        send_message(ecup_conn,
            Message::new_blocking_scalar(ecup::UpdateOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't quit updater thread");
        unsafe{xous::disconnect(ecup_conn).ok()};
    } */

    // check for backups after EC updates, but before we check for gateware updates
    let mut backup_time: Option<DateTime<Utc>> = None;
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
                    kbd.lock().unwrap().set_keymap(map).ok();

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
                            // once this step is done & successful, we have to erase the backup block to avoid
                            // re-doing this flow
                            keys.lock().unwrap().do_erase_backup();
                            log::info!("Rekey of PDDB to current device completed successfully")
                        }
                        Err(e) => {
                            modals
                                .show_notification(
                                    &format!("{}{:?}", t!("rekey.fail", locales::LANG), e),
                                    None,
                                )
                                .ok();
                            log::error!("Backup was aborted. Reason: {:?}", e);
                        }
                    }
                }
                BackupOp::Backup => {
                    // once we have unlocked the PDDB and know our timezone, we'll compare the embedded
                    // timestamp to our current time, and delete the backup if it's too
                    // old.
                    backup_time = Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                        NaiveDateTime::from_timestamp_opt(header.timestamp as i64, 0).unwrap(),
                        chrono::offset::Utc,
                    ));
                }
                _ => log::warn!(
                    "backup record was found, but it has an improper operation field: {:?}",
                    header.op
                ),
            }
        }
        _ => log::info!("No backup header found"), // no backup header found, continue with boot
    }

    // check for gateware updates
    let staged_sv = match keys.lock().unwrap().staged_semver() {
        Ok(sv) => {
            log::info!("Staged gateware version: {:?}", sv);
            Some(sv)
        }
        Err(xous::Error::InvalidString) => {
            log::info!("No staged gateware found; metadata is blank");
            None
        }
        _ => {
            log::error!("Internal error reading staged gateware semantic version");
            None
        }
    };
    if !keys.lock().unwrap().is_initialized().unwrap() {
        sec_notes
            .lock()
            .unwrap()
            .insert("secnotes.no_keys".to_string(), t!("secnote.no_keys", locales::LANG).to_string());
        if !restore_running {
            if let Some(staged) = staged_sv {
                let soc = llio.soc_gitrev().expect("error querying SoC gitrev; this is fatal");
                if staged > soc {
                    // we have a staged update, and no root keys. Just try to do the update.
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
                            t!("secnote.gateware_fail", locales::LANG).to_string(),
                        );
                    }
                } else {
                    let mut sn = clone.lock().unwrap();
                    sn.insert(
                        "secnotes.state_fail".to_string(),
                        t!("secnote.state_fail", locales::LANG).to_string(),
                    );
                }
            }
        });
        if let Some(staged) = staged_sv {
            let soc = llio.soc_gitrev().expect("error querying SoC gitrev; this is fatal");
            if (staged > soc) && !soc_updated {
                // if the soc was updated, we should reboot before we try this
                if keys.lock().unwrap().prompt_for_update() {
                    // prompt to apply the update
                    modals
                        .add_list_item(t!("rootkeys.gwup.yes", locales::LANG))
                        .expect("couldn't build radio item list");
                    modals
                        .add_list_item(t!("rootkeys.gwup.no", locales::LANG))
                        .expect("couldn't build radio item list");
                    modals
                        .add_list_item(t!("socup.ignore", locales::LANG))
                        .expect("couldn't build radio item list");
                    match modals.get_radiobutton(t!("socup.candidate", locales::LANG)) {
                        Ok(response) => {
                            if response.as_str() == t!("rootkeys.gwup.yes", locales::LANG) {
                                keys.lock().unwrap().do_update_gw_ux_flow_blocking();
                                soc_updated = true;
                            } else if response.as_str() == t!("socup.ignore", locales::LANG) {
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
    // now that all the auto-update interaction is done, set the EC is ready flag
    llio.set_ec_ready(true);
    // now that all the auto-update interaction is done, exit the gutter server. From
    // this point forward, messages will pile up in the status queue, until the main loop starts.
    send_message(cb_cid, Message::new_blocking_scalar(StatusOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't exit the gutter server");
    gutter.join().expect("status boot gutter server did not exit gracefully");
    // allocate some storage for backup checksums
    let checksums: Arc<Mutex<Option<root_keys::api::Checksums>>> = Arc::new(Mutex::new(None));

    // --------------------------- graphical loop timing
    let mut stats_phase: usize = 0;

    let charger_pump_interval = 180;
    let batt_interval;
    let secnotes_interval;
    if cfg!(feature = "braille") {
        // lower the status output rate for braille mode - it's invisible anyways, this is mainly for the
        // debug configuration
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
    com.hook_batt_stats(battstats_cb).expect("|status: couldn't hook callback for events from COM");
    // prime the loop
    com.req_batt_stats().expect("Can't get battery stats from COM");

    // ---------------------- final cleanup before entering main loop
    log::debug!("subscribe to wifi updates");
    netmgr.wifi_state_subscribe(cb_cid, StatusOpcode::WifiStats.to_u32().unwrap()).unwrap();
    let mut wifi_status: WlanStatus = WlanStatus::from_ipc(WlanStatusIpc::default());

    #[cfg(feature = "tts")]
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
    log::info!("|status: starting main loop"); // do not remove, this is used by the CI test infrastructure

    // add a security note if we're booting with a "zero key"
    match keys.lock().unwrap().is_zero_key().expect("couldn't query zero key status") {
        Some(q) => match q {
            true => {
                sec_notes.lock().unwrap().insert(
                    "secnote.zero_key".to_string(),
                    t!("secnote.zero_key", locales::LANG).to_string(),
                );
            }
            false => {}
        },
        None => {
            {} // could be bbram, could be an error. could be keys are secured and disabled for readout. could be disabled-readout zero keys!
        }
    }
    #[cfg(any(feature = "precursor", feature = "renode"))]
    llio.clear_wakeup_alarm().unwrap(); // this is here to clear any wake-up alarms that were set by a prior coldboot command

    // get the last status
    let must_sleep = early_settings.early_sleep().unwrap();

    if must_sleep {
        // reset it for good measure
        early_settings.set_early_sleep(false).unwrap();

        if !llio.is_plugged_in() {
            match susres.initiate_suspend() {
                Ok(_) => {}
                Err(xous::Error::Timeout) => {
                    // TODO: maybe this branch needs a different log message/flow?
                    modals.show_notification(t!("suspend.fail", locales::LANG), None).unwrap();
                }
                Err(_e) => {
                    panic!("Unhandled error on suspend request");
                }
            }
        }
    }

    // spawn a thread to auto-mount the PDDB
    let _ = thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let gam = gam::Gam::new(&xns).unwrap();
            while !gam.trusted_init_done().unwrap() {
                tt.sleep_ms(100).ok();
            }
            loop {
                let (no_retry_failure, count) = pddb::Pddb::new().try_mount();
                if no_retry_failure {
                    // this includes both successfully mounted, and user abort of mount attempt
                    break;
                } else {
                    // this indicates system was guttered due to a retry failure
                    let susres = susres::Susres::new_without_hook(&xns).unwrap();
                    let llio = llio::Llio::new(&xns);
                    if !llio.is_plugged_in() {
                        // try to force suspend if possible, so that users who are just playing around with
                        // the device don't run the battery down accidentally.
                        susres.initiate_suspend().ok();
                        tt.sleep_ms(1000).unwrap();
                        let modals = modals::Modals::new(&xns).unwrap();
                        modals
                            .show_notification(
                                &t!("login.fail", locales::LANG).replace("{fails}", &count.to_string()),
                                None,
                            )
                            .ok();
                    } else {
                        // otherwise force a reboot cycle to slow down guessers
                        susres.reboot(true).expect("Couldn't reboot after too many failed password attempts");
                        tt.sleep_ms(5000).unwrap();
                    }
                }
            }
        }
    });

    /*
    This thread handles preference loading.
    It'll wait until PDDB is ready to load stuff off the preference
    dictionary.
    */
    std::thread::spawn({
        let autosleep_duration_mins = autosleep_duration_mins.clone();
        let reboot_on_autosleep = reboot_on_autosleep.clone();
        let autobacklight_duration_secs = autobacklight_duration_secs.clone();
        move || {
            let pddb = pddb::Pddb::new();
            let prefs = prefs_thread_clone.lock().unwrap();
            let netmgr = net::NetManager::new();

            pddb.is_mounted_blocking();

            let all_prefs = match prefs.all() {
                Ok(p) => p,
                Err(error) => {
                    log::error!("cannot read preference store: {:?}", error);
                    return;
                }
            };

            log::debug!("pddb ready, loading preferences now!");

            match all_prefs.wifi_kill {
                true => netmgr.connection_manager_wifi_off_and_stop(),
                false => netmgr.connection_manager_wifi_on(),
            }
            .unwrap_or_else(|error| log::error!("cannot set radio status: {:?}", error));

            match prefs.connect_known_networks_on_boot_or_value(true).unwrap() {
                true => netmgr.connection_manager_run(),
                false => netmgr.connection_manager_stop(),
            }
            .unwrap_or_else(|error| log::error!("cannot start connection manager: {:?}", error));
            match prefs.autobacklight_on_boot_or_value(true).unwrap() {
                true => send_message(
                    status_cid,
                    Message::new_scalar(
                        StatusOpcode::EnableAutomaticBacklight.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ),
                false => send_message(
                    status_cid,
                    Message::new_scalar(
                        StatusOpcode::DisableAutomaticBacklight.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ),
            }
            .unwrap_or_else(|error| {
                log::error!("cannot set autobacklight status: {:?}", error);
                xous::Result::Ok
            });

            // keyboard mapping is restored directly by the keyboard hardware
            #[cfg(not(feature = "no-codec"))]
            {
                log::info!("audio enable state: {}", all_prefs.audio_enabled);
                #[cfg(feature = "tts")]
                // if TTS is on, never disable audio system, and don't allow 0-volume for audio
                {
                    codec.setup_8k_stream().ok();
                    send_message(
                        prefs_cid,
                        Message::new_scalar(
                            PrefsMenuUpdateOp::UpdateMenuAudioDisabled.to_usize().unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .unwrap();
                    let hp_percentage = if all_prefs.headset_volume == 0 {
                        log::warn!("Attempted to set headphone volume to 0, disallowing in TTS mode");
                        100
                    } else {
                        all_prefs.headset_volume
                    };
                    let spk_percentage = if all_prefs.earpiece_volume == 0 {
                        log::warn!("Attempted to set speaker volume to 0, disallowing in TTS mode");
                        100
                    } else {
                        all_prefs.earpiece_volume
                    };
                    codec
                        .set_headphone_volume(
                            codec::VolumeOps::Set,
                            Some(percentage_to_db(hp_percentage) as f32),
                        )
                        .unwrap_or_else(|error| {
                            log::error!("cannot set headphone volume: {:?}", error);
                        });

                    codec
                        .set_speaker_volume(
                            codec::VolumeOps::Set,
                            Some(percentage_to_db(spk_percentage) as f32),
                        )
                        .unwrap_or_else(|error| {
                            log::error!("cannot set speaker volume: {:?}", error);
                        });
                }
                #[cfg(not(feature = "tts"))]
                {
                    match all_prefs.audio_enabled {
                        true => match codec.setup_8k_stream() {
                            Ok(()) => {
                                send_message(
                                    prefs_cid,
                                    Message::new_scalar(
                                        PrefsMenuUpdateOp::UpdateMenuAudioDisabled.to_usize().unwrap(),
                                        0,
                                        0,
                                        0,
                                        0,
                                    ),
                                )
                                .unwrap();
                                Ok(())
                            }
                            Err(e) => Err(e),
                        },
                        false => Ok(()),
                    }
                    .unwrap_or_else(|error| {
                        log::error!("cannot set audio enabled: {:?}", error);
                    });

                    codec
                        .set_headphone_volume(
                            codec::VolumeOps::Set,
                            Some(percentage_to_db(all_prefs.headset_volume) as f32),
                        )
                        .unwrap_or_else(|error| {
                            log::error!("cannot set headphone volume: {:?}", error);
                        });

                    codec
                        .set_speaker_volume(
                            codec::VolumeOps::Set,
                            Some(percentage_to_db(all_prefs.earpiece_volume) as f32),
                        )
                        .unwrap_or_else(|error| {
                            log::error!("cannot set speaker volume: {:?}", error);
                        });
                }
            }
            autosleep_duration_mins
                .store(prefs.autosleep_timeout_or_value(0).unwrap() as u32, Ordering::SeqCst);
            reboot_on_autosleep.store(prefs.reboot_on_autosleep_or_value(false).unwrap(), Ordering::SeqCst);
            autobacklight_duration_secs
                .store(prefs.autobacklight_timeout_or_value(10).unwrap() as u32, Ordering::SeqCst);
        }
    });

    // this thread handles updating the PDDB basis list
    thread::spawn({
        let sec_notes = sec_notes.clone();
        move || {
            let pddb = pddb::Pddb::new();
            loop {
                // this blocks until there is a change in the basis list
                let mut basis_list_vec = pddb.monitor_basis();

                // the key may or may not be there, but remove it in case it is
                sec_notes.lock().unwrap().remove(&"secnote.basis".to_string());

                // only if there are more bases open than just the .System basis, insert a new key
                if basis_list_vec.len() > 1 {
                    let mut new_list_str = t!("secnote.basis", locales::LANG).to_string();
                    // initially, just concatenate all the basis names...
                    basis_list_vec.reverse(); // reverse the order so the highest priority basis is on the left.
                    for basis in basis_list_vec {
                        new_list_str.push_str(&basis);
                        if basis != pddb::PDDB_DEFAULT_SYSTEM_BASIS {
                            new_list_str.push_str(" > ");
                        }
                    }

                    sec_notes.lock().unwrap().insert("secnote.basis".to_string(), new_list_str);
                }
            }
        }
    });

    // storage for wifi bars
    let mut wifi_bars: [PixelColor; 5] =
        [PixelColor::Light, PixelColor::Light, PixelColor::Light, PixelColor::Light, PixelColor::Light];

    pump_run.store(true, Ordering::Relaxed); // start status thread updating
    loop {
        let msg = xous::receive_message(status_sid).unwrap();
        let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(StatusOpcode::ReloadPrefs) => {
                let p = prefs.lock().unwrap(); // lock it once in this block
                autosleep_duration_mins
                    .store(p.autosleep_timeout_or_value(0).unwrap() as u32, Ordering::SeqCst);
                reboot_on_autosleep.store(p.reboot_on_autosleep_or_value(false).unwrap(), Ordering::SeqCst);
                autobacklight_duration_secs
                    .store(p.autobacklight_timeout_or_value(10).unwrap() as u32, Ordering::SeqCst);
            }
            Some(StatusOpcode::EnableAutomaticBacklight) => {
                if *autobacklight_enabled.lock().unwrap() {
                    // already enabled, don't re-enable
                    continue;
                }
                *autobacklight_enabled.lock().unwrap() = true;

                // second: delete the first three elements off the menu
                menu_manager.delete_item(t!("mainmenu.backlighton", locales::LANG));
                menu_manager.delete_item(t!("mainmenu.backlightoff", locales::LANG));
            }
            Some(StatusOpcode::DisableAutomaticBacklight) => {
                if !(*autobacklight_enabled.lock().unwrap()) {
                    // already disabled, don't re-disable
                    continue;
                }
                *autobacklight_enabled.lock().unwrap() = false;
                tx.send(BacklightThreadOps::Stop).unwrap();

                // third: construct an array of the new elements to add to the menu.
                let new_elems = [
                    gam::MenuItem {
                        name: String::from(t!("mainmenu.backlighton", locales::LANG)),
                        action_conn: Some(com.conn()),
                        action_opcode: com.getop_backlight(),
                        action_payload: gam::MenuPayload::Scalar([191 >> 3, 191 >> 3, 0, 0]),
                        close_on_select: true,
                    },
                    gam::MenuItem {
                        name: String::from(t!("mainmenu.backlightoff", locales::LANG)),
                        action_conn: Some(com.conn()),
                        action_opcode: com.getop_backlight(),
                        action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                ];

                new_elems.iter().enumerate().for_each(|(index, element)| {
                    let _ = menu_manager.insert_item(element.clone(), index);
                });
            }
            Some(StatusOpcode::BattStats) => msg_scalar_unpack!(msg, lo, hi, _, _, {
                stats = [lo, hi].into();
                // have to clear the entire rectangle area, because the SSID has a variable width and can be
                // much wider or shorter than battstats
                gam.draw_rectangle(status_gid, stats_rect).ok();

                let battstats = if !battstats_phase && wifi_status.ssid.is_some() {
                    // move the SSID name 30 pixels to the left only if there's a link
                    Point { x: stats_rect.tr().x - 30, y: stats_rect.tr().y }
                } else {
                    Point { x: stats_rect.tr().x, y: stats_rect.tr().y }
                };
                // build battstats text view: right half of status bar
                let mut battstats_tv =
                    TextView::new(status_gid, TextBounds::GrowableFromTr(battstats, stats_rect.width() as _));
                battstats_tv.style = GlyphStyle::Regular;
                battstats_tv.draw_border = false;
                battstats_tv.margin = Point::new(0, 0);
                gam.post_textview(&mut battstats_tv).expect("|status: can't draw battery stats");

                // 0xdddd and 0xffff are what are returned when the EC is too busy to respond/hung, or in
                // reset, respectively
                if stats.current == -8739 /* 0xdddd */
                || stats.voltage == 0xdddd || stats.voltage == 0xffff
                || stats.soc == 0xdd || stats.soc == 0xff
                {
                    write!(&mut battstats_tv, "{}", t!("stats.measuring", locales::LANG)).unwrap();
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
                        write!(
                            &mut battstats_tv,
                            "{}.{:02}W{}{}.{:02}V {}%",
                            wattage_mw / 1000,
                            wattage_mw % 1000,
                            sign,
                            stats.voltage as u32 / 1000,
                            (stats.voltage as u32 % 1000) / 10, // 2 decimal places
                            stats.soc
                        )
                        .unwrap();
                    } else {
                        if let Some(ssid) = &wifi_status.ssid {
                            log::debug!("RSSI: -{}dBm", ssid.rssi);
                            compute_bars(&mut wifi_bars, ssid.rssi);
                            bars(&gam, status_gid, &wifi_bars, Point { x: 310, y: 13 }, (3, 2), 3, 2);
                            write!(&mut battstats_tv, "{}", ssid.name.as_str(),).unwrap();
                        } else {
                            if wifi_status.link_state == com_rs::LinkState::ResetHold {
                                write!(&mut battstats_tv, "{}", t!("stats.wifi_off", locales::LANG)).unwrap();
                            } else {
                                write!(&mut battstats_tv, "{}", t!("stats.disconnected", locales::LANG))
                                    .unwrap();
                            }
                        }
                    }
                }
                gam.post_textview(&mut battstats_tv).expect("|status: can't draw battery stats");
                if let Some(bounds) = battstats_tv.bounds_computed {
                    if bounds.height() as i16 > screensize.y / 2 + 1 {
                        // the clipping rectangle limits the bounds to the overall height of the status area,
                        // so the overlap between status and secnotes must be managed
                        // within this server
                        log::info!("Status text overstepped its intended bound. Forcing secnotes redraw.");
                        secnotes_force_redraw = true;
                    }
                }
                battstats_phase = !battstats_phase;
            }),
            Some(StatusOpcode::WifiStats) => {
                let buffer =
                    unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                wifi_status = WlanStatus::from_ipc(buffer.to_original::<com::WlanStatusIpc, _>().unwrap());
            }
            Some(StatusOpcode::Preferences) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::PREFERENCES_MENU_NAME).unwrap();
            }
            Some(StatusOpcode::Pump) => {
                let elapsed_time = ticktimer.elapsed_ms();
                {
                    // update the CPU load bar
                    let mut draw_list = GamObjectList::new(status_gid);
                    draw_list.push(GamObjectType::Rect(cpuload_rect)).unwrap();
                    let (latest_activity, period) =
                        llio.activity_instantaneous().expect("couldn't get CPU activity");
                    let activity_to_width = if period == 0 {
                        cpuload_rect.width() as i16 - 4
                    } else {
                        (((latest_activity as u64) * 1000u64 * (cpuload_rect.width() as u64 - 4))
                            / (period as u64 * 1000u64)) as i16
                    };
                    draw_list
                        .push(GamObjectType::Rect(Rectangle::new_coords_with_style(
                            cpuload_rect.tl().x + 2,
                            cpuload_rect.tl().y + 2,
                            cpuload_rect.tl().x
                                + 2
                                + (activity_to_width).min(cpuload_rect.width() as i16 - 4),
                            cpuload_rect.br().y - 2,
                            DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 0),
                        )))
                        .unwrap();
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
                            sec_notes.lock().unwrap().remove(&"secnote.usb_unlock".to_string()); // this is the key, not the value to remove
                        } else {
                            sec_notes.lock().unwrap().insert(
                                "secnotes.usb_unlock".to_string(),
                                t!("secnote.usb_unlock", locales::LANG).to_string(),
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
                                last_sec_note_index = (last_sec_note_index + 1) % last_sec_note_size;
                                break;
                            }
                        }
                    } else {
                        write!(&mut security_tv, "{}", t!("secnote.allclear", locales::LANG)).unwrap();
                    }

                    secnotes_force_redraw = false;
                    gam.post_textview(&mut security_tv).unwrap();
                    gam.draw_line(
                        status_gid,
                        Line::new_with_style(
                            Point::new(0, screensize.y),
                            screensize,
                            DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
                        ),
                    )
                    .unwrap();
                }
                if (stats_phase % batt_interval) == (batt_interval - 1) {
                    com.req_batt_stats().expect("Can't get battery stats from COM");
                }
                if (stats_phase % charger_pump_interval) == 1 {
                    // stagger periodic tasks
                    // confirm that the charger is in the right state.
                    if stats.soc < 95 || stats.remaining_capacity < 1000 {
                        // only request if we aren't fully charged, either by SOC or capacity metrics
                        if llio.is_plugged_in() {
                            // 4.45V is our threshold for deciding if a cable is present
                            // charging cable is present
                            if !com.is_charging().expect("couldn't check charging state") {
                                // not charging, but cable is present
                                log::debug!(
                                    "Charger present, but not currently charging. Automatically requesting charge start."
                                );
                                com.request_charging().expect("couldn't send charge request");
                            }
                        }
                    }
                }
                {
                    // update the time field
                    // have to clear the entire rectangle area, because the text has a variable width and
                    // dirty text will remain if the text is shortened
                    gam.draw_rectangle(status_gid, time_rect).ok();
                    uptime_tv.clear_str();
                    if let Some(timestamp) = localtime.get_local_time_ms() {
                        // we "say" UTC but actually local time is in whatever the local time is
                        let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                            NaiveDateTime::from_timestamp_opt(timestamp as i64 / 1000, 0).unwrap(),
                            chrono::offset::Utc,
                        );
                        let timestr = dt.format("%H:%M %m/%d").to_string();
                        // TODO: convert dt to an actual local time using the chrono library
                        write!(&mut uptime_tv, "{}", timestr).unwrap();
                        if let Some(bt) = backup_time {
                            let since_backup = dt.signed_duration_since(bt);
                            if since_backup.num_hours().abs() > BACKUP_EXPIRATION_HOURS {
                                keys.lock().unwrap().do_erase_backup();
                                backup_time = None;
                            }
                        }
                    } else {
                        if pddb_poller.is_mounted_nonblocking() {
                            write!(&mut uptime_tv, "{}", t!("stats.set_time", locales::LANG)).unwrap();
                        } else {
                            write!(&mut uptime_tv, "{}", t!("stats.mount_pddb", locales::LANG)).unwrap();
                        }
                    }
                    // use ticktimer, not stats_phase, because stats_phase encodes some phase drift due to
                    // task-switching overhead
                    write!(
                        &mut uptime_tv,
                        " {}{}:{:02}:{:02}",
                        t!("stats.uptime", locales::LANG),
                        (elapsed_time / 3_600_000),
                        (elapsed_time / 60_000) % 60,
                        (elapsed_time / 1000) % 60,
                    )
                    .expect("|status: can't write string");
                    gam.post_textview(&mut uptime_tv).expect("|status: can't draw uptime");
                    if let Some(bounds) = uptime_tv.bounds_computed {
                        if bounds.height() as i16 > screensize.y / 2 + 1 {
                            // the clipping rectangle limits the bounds to the overall height of the status
                            // area, so the overlap between status and secnotes
                            // must be managed within this server
                            log::info!(
                                "Status text overstepped its intended bound. Forcing secnotes redraw."
                            );
                            secnotes_force_redraw = true;
                        }
                    }
                }
                log::trace!("status redraw## update");
                gam.redraw().expect("|status: couldn't redraw");

                stats_phase = stats_phase.wrapping_add(1);
            }
            Some(StatusOpcode::Reboot) => {
                // this is described as "Lock device" on the menu
                let pddb = pddb::Pddb::new();
                if !pddb.try_unmount() {
                    // sync the pddb prior to lock
                    modals.show_notification(t!("socup.unmount_fail", locales::LANG), None).ok();
                } else {
                    early_settings.set_early_sleep(true).unwrap();

                    log::info!("forcing sleep on reboot");

                    pddb.pddb_halt();
                    susres.reboot(true).expect("couldn't issue reboot command");
                }
            }
            Some(StatusOpcode::SubmenuPddb) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::PDDB_MENU_NAME).expect("couldn't raise PDDB submenu");
            }
            Some(StatusOpcode::SubmenuApp) => {
                ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close
                gam.raise_menu(gam::APP_MENU_NAME).expect("couldn't raise App submenu");
            }
            Some(StatusOpcode::SwitchToShellchat) => {
                ticktimer.sleep_ms(100).ok();
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes
                    .lock()
                    .unwrap()
                    .insert("current_app".to_string(), format!("Running: Shellchat").to_string());
                gam.switch_to_app(gam::APP_NAME_SHELLCHAT, security_tv.token.unwrap())
                    .expect("couldn't raise shellchat");
                secnotes_force_redraw = true;
                send_message(cb_cid, Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0))
                    .expect("couldn't trigger status update");
            }
            Some(StatusOpcode::SwitchToApp) => msg_scalar_unpack!(msg, index, _, _, _, {
                ticktimer.sleep_ms(100).ok();
                let app_name = app_autogen::app_index_to_name(index).expect("app index not found");
                app_autogen::app_dispatch(&gam, security_tv.token.unwrap(), index)
                    .expect("cannot switch to app");
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes
                    .lock()
                    .unwrap()
                    .insert("current_app".to_string(), format!("Running: {}", app_name).to_string());
                secnotes_force_redraw = true;
                send_message(cb_cid, Message::new_scalar(StatusOpcode::Pump.to_usize().unwrap(), 0, 0, 0, 0))
                    .expect("couldn't trigger status update");
            }),
            Some(StatusOpcode::TrySuspend) => {
                if llio.is_plugged_in() {
                    modals
                        .show_notification(t!("mainmenu.cant_sleep", locales::LANG), None)
                        .expect("couldn't notify that power is plugged in");
                } else {
                    // reset the last key hit timer, so that when we wake up we get a full timeout period
                    last_key_hit_secs.store((ticktimer.elapsed_ms() / 1000) as u32, Ordering::SeqCst);
                    // log::set_max_level(log::LevelFilter::Debug);
                    match susres.initiate_suspend() {
                        Ok(_) => {}
                        Err(xous::Error::Timeout) => {
                            modals.show_notification(t!("suspend.fail", locales::LANG), None).unwrap();
                        }
                        Err(_e) => {
                            panic!("Unhandled error on suspend request");
                        }
                    }
                }
            }
            Some(StatusOpcode::BatteryDisconnect) => {
                // this is described as "Shutdown" on the menu
                // NOTE: this implementation takes a "shortcut" and blocks, which causes the
                // status thread to block while the dialog boxes are up. This can eventually lead
                // to deadlock in the system, but we assume that the user will acknowledge these
                // dialog boxes fairly quickly. If this turns out not to be the case, we can
                // turn the interactive dialog box into a thread that fires a message to move
                // to the next stage (see `PrepareBackup` implementation for a template).
                if llio.is_plugged_in() {
                    modals
                        .show_notification(t!("mainmenu.cant_sleep", locales::LANG), None)
                        .expect("couldn't notify that power is plugged in");
                } else {
                    // show a note to inform the user that you can't turn it on without an external power
                    // source...
                    modals
                        .add_list_item(t!("rootkeys.gwup.yes", locales::LANG))
                        .expect("couldn't build radio item list");
                    modals
                        .add_list_item(t!("rootkeys.gwup.no", locales::LANG))
                        .expect("couldn't build radio item list");
                    match modals.get_radiobutton(t!("mainmenu.shutdown_confirm", locales::LANG)) {
                        Ok(response) => {
                            if response.as_str() == t!("rootkeys.gwup.yes", locales::LANG) {
                                {}
                            } else {
                                // abort the flow now by returning to the main dispatch handler
                                continue;
                            }
                        }
                        _ => (),
                    }
                    // unmount things before shutting down
                    let pddb = pddb::Pddb::new();
                    if !pddb.try_unmount() {
                        modals.show_notification(t!("socup.unmount_fail", locales::LANG), None).ok();
                    } else {
                        pddb.pddb_halt();
                        gam.shipmode_blank_request().ok();
                        ticktimer.sleep_ms(500).unwrap();
                        llio.allow_ec_snoop(true).unwrap();
                        llio.allow_power_off(true).unwrap();
                        com.ship_mode().unwrap();
                        susres.immediate_poweroff().unwrap();
                    }
                }
            }

            Some(StatusOpcode::Keypress) => {
                // this will roll over in 126 years of uptime. meh?
                last_key_hit_secs.store((ticktimer.elapsed_ms() / 1000) as u32, Ordering::SeqCst);

                if !*autobacklight_enabled.lock().unwrap() {
                    log::trace!("ignoring keypress, automatic backlight is disabled");
                    continue;
                }
                let mut run_lock = autobacklight_thread_already_running.lock().unwrap();
                match *run_lock {
                    true => {
                        log::trace!("renewing backlight timer");
                        tx.send(BacklightThreadOps::Renew).unwrap();
                        continue;
                    }
                    false => {
                        *run_lock = true;

                        let abl_timeout = if pddb_poller.is_mounted_nonblocking() {
                            autobacklight_duration_secs.load(Ordering::SeqCst) as u64
                        } else {
                            // this routine can be polled before the pddb is mounted, e.g. while the pddb
                            // password is entered
                            10
                        };

                        com.set_backlight(255, 128).expect("cannot set backlight on");
                        std::thread::spawn({
                            let rx = rx.clone();
                            move || turn_lights_on(rx, thread_conn, abl_timeout)
                        });
                    }
                }
            }
            Some(StatusOpcode::TurnLightsOn) => {
                log::trace!("turning lights on");
                com.set_backlight(255, 128).expect("cannot set backlight on");
            }
            Some(StatusOpcode::TurnLightsOff) => {
                log::trace!("turning lights off");
                let mut run_lock = autobacklight_thread_already_running.lock().unwrap();
                *run_lock = false;
                com.set_backlight(0, 0).expect("cannot set backlight off");
            }
            #[cfg(feature = "efuse")]
            Some(StatusOpcode::BurnBackupKey) => {
                log::info!("{}BURNKEY.TYPE,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                thread::spawn({
                    let keys = keys.clone();
                    move || {
                        let xns = xous_names::XousNames::new().unwrap();
                        let modals = modals::Modals::new(&xns).unwrap();
                        modals
                            .add_list_item(t!("burnkey.bbram", locales::LANG))
                            .expect("couldn't build radio item list");
                        modals
                            .add_list_item(t!("burnkey.efuse", locales::LANG))
                            .expect("couldn't build radio item list");
                        modals
                            .add_list_item(t!("wlan.cancel", locales::LANG))
                            .expect("couldn't build radio item list");
                        match modals.get_radiobutton(t!("burnkey.type", locales::LANG)) {
                            Ok(response) => {
                                if response.as_str() == t!("burnkey.bbram", locales::LANG) {
                                    // do BBRAM flow
                                    log::info!("{}BBRAM.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                    // punts to a script-driven flow on the pi
                                    modals.show_notification(
                                        t!("burnkey.bbram_exec", locales::LANG),
                                        Some("https://github.com/betrusted-io/betrusted-wiki/wiki/FAQ:-FPGA-AES-Encryption-Key-(eFuse-BBRAM)"),
                                    ).ok();
                                } else if response.as_str() == t!("burnkey.efuse", locales::LANG) {
                                    log::info!("{}EFUSE.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                    // do eFuse flow
                                    modals
                                        .add_list_item(t!("rootkeys.gwup.yes", locales::LANG))
                                        .expect("couldn't build radio item list");
                                    modals
                                        .add_list_item(t!("rootkeys.gwup.no", locales::LANG))
                                        .expect("couldn't build radio item list");
                                    match modals.get_radiobutton(t!("burnkey.efuse_confirm", locales::LANG)) {
                                        Ok(response) => {
                                            if response.as_str() == t!("rootkeys.gwup.yes", locales::LANG) {
                                                // do the efuse burn
                                                keys.lock().unwrap().do_efuse_burn();
                                            } else {
                                                // abort the flow by doing nothing
                                            }
                                        }
                                        _ => (),
                                    }
                                } else {
                                    // abort flow by doing nothing
                                }
                            }
                            _ => (),
                        }
                    }
                });
            }
            Some(StatusOpcode::PrepareBackup) => {
                // don't block while prompting for backup confirmation
                thread::spawn({
                    move || {
                        let xns = xous_names::XousNames::new().unwrap();
                        let modals = modals::Modals::new(&xns).unwrap();
                        modals
                            .add_list_item(t!("rootkeys.gwup.yes", locales::LANG))
                            .expect("couldn't build radio item list");
                        modals
                            .add_list_item(t!("rootkeys.gwup.no", locales::LANG))
                            .expect("couldn't build radio item list");
                        log::info!("{}BACKUP.CONFIRM,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        match modals.get_radiobutton(t!("backup.confirm", locales::LANG)) {
                            Ok(response) => {
                                if response.as_str() == t!("rootkeys.gwup.yes", locales::LANG) {
                                    send_message(
                                        cb_cid,
                                        Message::new_scalar(
                                            StatusOpcode::PrepareBackupConfirmed.to_usize().unwrap(),
                                            0,
                                            0,
                                            0,
                                            0,
                                        ),
                                    )
                                    .expect("couldn't initiate backup");
                                } else {
                                    // abort by just falling through
                                }
                            }
                            _ => (),
                        }
                    }
                });
            }
            Some(StatusOpcode::PrepareBackupConfirmed) => {
                // disconnect from the network, so that incoming network packets don't trigger any processes
                // that could write to the PDDB.
                netmgr.connection_manager_wifi_off_and_stop().ok();

                // close the active app and switch to shellchat
                ticktimer.sleep_ms(100).ok();
                sec_notes.lock().unwrap().remove(&"current_app".to_string());
                sec_notes
                    .lock()
                    .unwrap()
                    .insert("current_app".to_string(), format!("Running: Shellchat").to_string());
                gam.switch_to_app(gam::APP_NAME_SHELLCHAT, security_tv.token.unwrap())
                    .expect("couldn't raise shellchat");
                secnotes_force_redraw = true;

                // unmount the PDDB and compute a checksum; then halt the PDDB to freeze its state
                thread::spawn({
                    // thread the checksum process, because it can take a long time.
                    // we might also want to make it optional down the road, in case people want a "fast"
                    // backup option
                    let checksums = checksums.clone();
                    move || {
                        // sync the PDDB to disk prior to making backups
                        let pddb = pddb::Pddb::new();
                        if !pddb.try_unmount() {
                            let xns = xous_names::XousNames::new().unwrap();
                            let modals = modals::Modals::new(&xns).unwrap();
                            modals.show_notification(t!("socup.unmount_fail", locales::LANG), None).ok();
                        } else {
                            *checksums.lock().unwrap() = Some(pddb.compute_checksums());
                            // PDDB is halted forever. However, the system is reset after a backup is run.
                            pddb.pddb_halt();

                            // now trigger phase 2 of the backup
                            log::info!("{}BACKUP.PHASE2,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                            send_message(
                                cb_cid,
                                Message::new_scalar(
                                    StatusOpcode::PrepareBackupPhase2.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .expect("couldn't initiate phase 2 backup");
                        }
                    }
                });
            }
            Some(StatusOpcode::PrepareBackupPhase2) => {
                let mut metadata = root_keys::api::BackupHeader::default();
                // note: default() should set the language correctly by default since it's a systemwide
                // constant
                metadata.timestamp = localtime.get_local_time_ms().unwrap_or(0);
                metadata.xous_ver = ticktimer.get_version_semver().into();
                metadata.soc_ver = llio.soc_gitrev().unwrap().into();
                metadata.wf200_ver = com.get_wf200_fw_rev().unwrap().into();
                metadata.ec_ver = com.get_ec_sw_tag().unwrap().into();
                metadata.op = BackupOp::Backup;
                metadata.dna = llio.soc_dna().unwrap().to_le_bytes();
                let map = kbd.lock().unwrap().get_keymap().expect("couldn't get key mapping");
                let map_serialize: BackupKeyboardLayout = map.into();
                metadata.kbd_layout = map_serialize.into();
                // the backup process is coded to accept the option of no checksums, but the UX currently
                // always computes it.
                if let Some(checksums) = checksums.lock().unwrap().take() {
                    keys.lock().unwrap().do_create_backup_ux_flow(metadata, Some(checksums));
                } else {
                    keys.lock().unwrap().do_create_backup_ux_flow(metadata, None);
                }
            }
            Some(StatusOpcode::ForceEcUpdate) => {
                // wrap in thread so the status loop doesn't crash due to event back-pressure during the
                // potentially very long running EC update process (which blocks the event
                // handler while it runs)...
                thread::spawn({
                    let ecup_conn = ecup_conn.clone();
                    move || {
                        // send with an argument of '1', which forces the update
                        match send_message(
                            ecup_conn,
                            Message::new_blocking_scalar(
                                ecup::UpdateOp::UpdateAuto.to_usize().unwrap(),
                                1,
                                0,
                                0,
                                0,
                            ),
                        )
                        .expect("couldn't send auto update command")
                        {
                            xous::Result::Scalar1(r) => {
                                match FromPrimitive::from_usize(r) {
                                    Some(ecup::UpdateResult::AutoDone) => {
                                        let xns = xous_names::XousNames::new().unwrap();
                                        let llio = llio::Llio::new(&xns);
                                        let netmgr = net::NetManager::new();
                                        // restore interrupts and connection manager
                                        llio.com_event_enable(true).ok();
                                        netmgr.reset();
                                    }
                                    Some(ecup::UpdateResult::NothingToDo) => {
                                        log::error!(
                                            "Got 'NothingToDo' on force argument. This is a hard error."
                                        );
                                        panic!(
                                            "Force update responded with nothing to do. This is a bug, please report it in xous-core issues, and note the results of `ver ec`, `ver xous`, `ver soc`, `ver wf200`."
                                        );
                                    }
                                    Some(ecup::UpdateResult::Abort) => {
                                        let xns = xous_names::XousNames::new().unwrap();
                                        let modals = modals::Modals::new(&xns).unwrap();
                                        modals
                                            .show_notification(t!("ecup.abort", locales::LANG), None)
                                            .unwrap();
                                    }
                                    // note: invalid package triggers a pop-up in the update procedure, so we
                                    // don't need to pop one up here.
                                    Some(ecup::UpdateResult::PackageInvalid) => {
                                        log::error!("EC firmware package did not validate")
                                    }
                                    None => log::error!("invalid return code from EC update check"),
                                }
                            }
                            _ => log::error!("Invalid return type from UpdateAuto"),
                        }
                    }
                });
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

fn turn_lights_on(rx: Box<Receiver<BacklightThreadOps>>, cid: xous::CID, timeout: u64) {
    let standard_duration = std::time::Duration::from_secs(timeout);

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

/// Returns true if the color changed
fn color_at_thresh(bar: &mut PixelColor, rssi: i32, threshold: i32) -> bool {
    if rssi < threshold {
        if *bar != PixelColor::Light {
            *bar = PixelColor::Light;
            true
        } else {
            false
        }
    } else {
        if *bar != PixelColor::Dark {
            *bar = PixelColor::Dark;
            true
        } else {
            false
        }
    }
}
/// Acconding to the WF200 datasheet, -91.6dBm is the cutoff for 6Mbps 802.11g reception @ 10% PER, and
/// -74.8dBm is the cutoff for 54Mbps @ 10% PER. The saturation point is -9 dBm, at which point the LNAs
/// fail due to too much signal.
///
/// Thus the scale should go from -91.6dBm to -9dBm, with -74.8dBm being "four bars". Above -74.8dBm
/// any extra signal does not improve your data rate, but we use the "fifth bar" to indicate
/// that you have extra margin on your singal.
///
/// -92dBm  - 0 bars - 6Mbps @ 10% packet error rate
/// -87dBm  - 1 bar - 6Mbps sustainable
/// -82dBm  - 2 bars
/// -77dBm  - 3 bars
/// -72dBm  - 4 bars - 54Mbps with 10% packet error rate
/// -60dBm+ - 5 bars - 54Mbps sustainable
///
/// Returns `true` if the bar list has changed; `false` if there is no change.
fn compute_bars(wifi_bars: &mut [PixelColor; 5], rssi: u8) -> bool {
    log::debug!("Rssi: -{}dBm, Bars before: {:?}", rssi, wifi_bars);
    let rssi_int: i32 = -(rssi as i32);
    let mut changed = false;
    for (index, bar) in wifi_bars.iter_mut().enumerate() {
        match index {
            // anything less than -87dBm shows up as 0 bars
            0 => changed |= color_at_thresh(bar, rssi_int, -87),
            1 => changed |= color_at_thresh(bar, rssi_int, -82),
            2 => changed |= color_at_thresh(bar, rssi_int, -77),
            3 => changed |= color_at_thresh(bar, rssi_int, -72),
            4 => changed |= color_at_thresh(bar, rssi_int, -60),
            _ => panic!("Should be unreachable; wifi_bars list is too long!"),
        }
    }
    log::debug!("New bars: {:?} ({:?})", wifi_bars, changed);
    changed
}
/// bars draws signal bars spaced by `bars_spacing` amount, drawing a
/// the smallest signal square starting from `top_left` growing by `growth` amount of pixels.
/// The smallest bar will be exactly `(x, y)` in size.
fn bars(
    g: &gam::Gam,
    canvas: graphics_server::Gid,
    levels: &[PixelColor; 5],
    top_left: Point,
    (x, y): (i16, i16),
    growth: i16,
    bars_spacing: i16,
) {
    let mut dl = gam::GamObjectList::new(canvas);

    let bottom_right = Point { x: top_left.x + x, y: top_left.y + y };

    for (index, bar) in levels.iter().enumerate() {
        let color = DrawStyle::new(*bar, PixelColor::Dark, 1);

        let r = match index {
            0 => gam::Rectangle::new_coords_with_style(
                top_left.x,
                top_left.y,
                bottom_right.x as i16,
                bottom_right.y,
                color,
            ),
            _ => {
                let last = match dl.last().unwrap() {
                    gam::GamObjectType::Rect(r) => r,
                    _ => panic!("expected only rects, found other stuff"),
                };

                gam::Rectangle::new_coords_with_style(
                    last.x1() as i16 + bars_spacing,
                    last.y0() as i16 - growth,
                    last.x1() as i16 + growth + bars_spacing,
                    bottom_right.y,
                    color,
                )
            }
        };

        dl.push(gam::GamObjectType::Rect(r)).unwrap();
    }

    g.draw_list(dl).unwrap();
}
