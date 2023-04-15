use num_traits::*;
use keyboard::{RowCol, KeyRawStates};
use core::sync::atomic::{AtomicU32, Ordering, AtomicBool};
use std::sync::Arc;

pub(crate) const SERVER_NAME_OQC: &str     = "_Outgoing Quality Check Test Program_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum OqcOp {
    Trigger,
    KeyCode,
    Status,
    UxGutter,
    ModalRedraw,
    ModalKeys,
    ModalDrop,
    Quit,
}

static SERVER_STARTED: AtomicBool = AtomicBool::new(false);
pub(crate) fn oqc_test(oqc_cid: Arc<AtomicU32>, kbd: keyboard::Keyboard) {
    // only start the server once!
    if SERVER_STARTED.load(Ordering::SeqCst) {
        return
    }
    SERVER_STARTED.store(true, Ordering::SeqCst);

    let xns = xous_names::XousNames::new().unwrap();
    // we allow any connections because this server is not spawned until it is needed
    let oqc_sid = xns.register_name(SERVER_NAME_OQC, None).expect("can't register server");

    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    //let kbd = keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_raw_listener(
        SERVER_NAME_OQC,
        OqcOp::KeyCode.to_usize().unwrap()
    );
    let com = com::Com::new(&xns).unwrap();
    let llio = llio::Llio::new(&xns);
    let gam = gam::Gam::new(&xns).unwrap();

    let modal = modals::Modals::new(&xns).unwrap();
    let mut test_run = false;
    let mut remaining = populate_vectors();
    let mut bot_str = String::new();
    let mut start_time = 0;
    let mut timeout = 120_000;
    let mut passing: Option<bool> = None;
    let mut test_finished = false;
    let mut last_redraw_time = 0;

    // this connection unblocks the calling thread
    oqc_cid.store(xous::connect(oqc_sid).unwrap(), Ordering::SeqCst);
    loop {
        let msg = xous::receive_message(oqc_sid).unwrap();
        let opcode: Option<OqcOp> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(OqcOp::Trigger) => xous::msg_blocking_scalar_unpack!(msg, timeout_set, _, _, _, {
                if !test_run {
                    // test the screen
                    com.set_backlight(255, 255).unwrap();
                    gam.selftest(8_000); // 12_000 by default

                    // now start the keyboard test
                    timeout = if timeout_set > 120_000 {
                        120_000
                    } else {
                        timeout_set as u64
                    };
                    start_time = ticktimer.elapsed_ms();
                    last_redraw_time = start_time;
                    log::info!("raising modal");
                    render_string(&mut bot_str, &remaining, timeout - (ticktimer.elapsed_ms() - start_time));
                    modal.dynamic_notification(None, Some(bot_str.as_str())).expect("couldn't raise test modal");

                    // start a thread that advances the timer when not hitting keys
                    xous::create_thread_2(ping_thread, xous::connect(oqc_sid).unwrap() as usize, timeout as usize).unwrap();
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
                test_run = true;
            }),
            Some(OqcOp::KeyCode) => {
                if test_run {
                    if test_finished {
                        // we'll continue to get keycodes, but ignore them once the test is finished
                        continue;
                    }
                    let elapsed = ticktimer.elapsed_ms();
                    if elapsed - start_time < timeout {
                        let buffer = unsafe {
                            xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap())
                        };
                        let krs = buffer.to_original::<[(u8, u8); 32],_>().unwrap();
                        let mut rawstates = KeyRawStates::new();
                        for &(r, c) in krs[..16].iter() {
                            if r != 255 || c != 255 {
                                rawstates.keydowns.push(RowCol{r, c});
                            }
                        }
                        for &(r, c) in krs[16..].iter() {
                            if r!= 255 || c != 255 {
                                rawstates.keyups.push(RowCol{r, c});
                            }
                        }

                        if rawstates.keydowns.len() > 0 { // only worry about keydowns
                            for &key in rawstates.keydowns.iter() {
                                for (rc, hit) in remaining.iter_mut() {
                                    if *rc == key {
                                        *hit = true;
                                    }
                                }
                            }
                            if elapsed - last_redraw_time > 100 { // rate limit redraws to 10Hz
                                render_string(&mut bot_str, &remaining, timeout - (elapsed - start_time));
                                modal.dynamic_notification_update(None, Some(bot_str.as_str())).expect("couldn't update test modal");
                                last_redraw_time = elapsed;
                                llio.vibe(llio::VibePattern::Short).unwrap();
                            }
                        }

                        // iterate and see if all keys have been hit
                        let mut finished = true;
                        for &(_rc, vals) in remaining.iter() {
                            if vals == false {
                                finished = false;
                                break;
                            }
                        }
                        if finished {
                            passing = Some(true);
                            com.set_backlight(0, 0).unwrap();
                            modal.dynamic_notification_close().unwrap();
                            ticktimer.sleep_ms(50).unwrap();
                            log::info!("all keys hit, exiting");
                            test_finished = true;
                        }
                    } else {
                        // timeout
                        passing = Some(false);
                        com.set_backlight(0, 0).unwrap();
                        modal.dynamic_notification_close().unwrap();
                        ticktimer.sleep_ms(50).unwrap();
                        log::info!("test done, relinquishing focus");
                        test_finished = true;
                    }
                } else {
                    // simply ignore the reports in
                    // we have to register our key listener early on, otherwise the rootkeys won't work for normal use
                    // we do want the keyboard listener slots to be fully occupied, otherwise something nefarious could
                    // squat the unused port...
                }
            },
            Some(OqcOp::Status) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let _ = match passing {
                    None => xous::return_scalar(msg.sender, 0),
                    Some(true) => xous::return_scalar(msg.sender, 1),
                    Some(false) => xous::return_scalar(msg.sender, 2),
                };
            }),
            Some(OqcOp::UxGutter) => {
                // log::info!("gutter");
                // an intentional NOP for UX actions that require a destintation but need no action
            },
            Some(OqcOp::ModalRedraw) => {
                // log::info!("modal redraw handler");
                // test_modal.redraw();
            },
            Some(OqcOp::ModalKeys) => xous::msg_scalar_unpack!(msg, _k1, _k2, _k3, _k4, {
                // log::info!("modal keys message, ignoring");
                // ignore keys, we have our own key routine
            }),
            Some(OqcOp::ModalDrop) => {
                log::error!("test modal quit unexpectedly");
            }
            Some(OqcOp::Quit) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                log::warn!("Quit received on OQC");
                xous::return_scalar(msg.sender, 1).unwrap();
                break;
            }),
            None => {
                log::error!("couldn't convert OqcOp: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(oqc_sid).unwrap();
    xous::destroy_server(oqc_sid).unwrap();
    log::trace!("quitting oqc server");
}

fn populate_vectors() -> Vec::<(RowCol, bool)> {
    let mut vectors = Vec::<(RowCol, bool)>::new();
    vectors.push((RowCol::new(0, 0), false));
    vectors.push((RowCol::new(0, 1), false));
    vectors.push((RowCol::new(0, 2), false));
    vectors.push((RowCol::new(0, 3), false));
    vectors.push((RowCol::new(0, 4), false));
    vectors.push((RowCol::new(4, 5), false));
    vectors.push((RowCol::new(4, 6), false));
    vectors.push((RowCol::new(4, 7), false));
    vectors.push((RowCol::new(4, 8), false));
    vectors.push((RowCol::new(4, 9), false));
    vectors.push((RowCol::new(1, 0), false));
    vectors.push((RowCol::new(1, 1), false));
    vectors.push((RowCol::new(1, 2), false));
    vectors.push((RowCol::new(1, 3), false));
    vectors.push((RowCol::new(1, 4), false));
    vectors.push((RowCol::new(5, 5), false));
    vectors.push((RowCol::new(5, 6), false));
    vectors.push((RowCol::new(5, 7), false));
    vectors.push((RowCol::new(5, 8), false));
    vectors.push((RowCol::new(5, 9), false));
    vectors.push((RowCol::new(2, 0), false));
    vectors.push((RowCol::new(2, 1), false));
    vectors.push((RowCol::new(2, 2), false));
    vectors.push((RowCol::new(2, 3), false));
    vectors.push((RowCol::new(2, 4), false));
    vectors.push((RowCol::new(6, 5), false));
    vectors.push((RowCol::new(6, 6), false));
    vectors.push((RowCol::new(6, 7), false));
    vectors.push((RowCol::new(6, 8), false));
    vectors.push((RowCol::new(6, 9), false));
    vectors.push((RowCol::new(3, 0), false));
    vectors.push((RowCol::new(3, 1), false));
    vectors.push((RowCol::new(3, 2), false));
    vectors.push((RowCol::new(3, 3), false));
    vectors.push((RowCol::new(3, 4), false));
    vectors.push((RowCol::new(7, 5), false));
    vectors.push((RowCol::new(7, 6), false));
    vectors.push((RowCol::new(7, 7), false));
    vectors.push((RowCol::new(7, 8), false));
    vectors.push((RowCol::new(7, 9), false));
    vectors.push((RowCol::new(8, 5), false));
    vectors.push((RowCol::new(8, 6), false));
    vectors.push((RowCol::new(8, 7), false));
    vectors.push((RowCol::new(8, 8), false));
    vectors.push((RowCol::new(8, 9), false));
    vectors.push((RowCol::new(8, 0), false));
    vectors.push((RowCol::new(8, 1), false));
    vectors.push((RowCol::new(3, 8), false));
    vectors.push((RowCol::new(3, 9), false));
    vectors.push((RowCol::new(8, 3), false));
    vectors.push((RowCol::new(3, 6), false));
    vectors.push((RowCol::new(6, 4), false));
    vectors.push((RowCol::new(8, 2), false));
    vectors.push((RowCol::new(5, 2), false));

    vectors
}

fn map_codes(code: RowCol) -> &'static str {
    let rc = (code.r, code.c);

    match rc {
        (0, 0) => "1",
        (0, 1) => "2",
        (0, 2) => "3",
        (0, 3) => "4",
        (0, 4) => "5",
        (4, 5) => "6",
        (4, 6) => "7",
        (4, 7) => "8",
        (4, 8) => "9",
        (4, 9) => "0",
        (1, 0) => "q",
        (1, 1) => "w",
        (1, 2) => "e",
        (1, 3) => "r",
        (1, 4) => "t",
        (5, 5) => "y",
        (5, 6) => "u",
        (5, 7) => "i",
        (5, 8) => "o",
        (5, 9) => "p",
        (2, 0) => "a",
        (2, 1) => "s",
        (2, 2) => "d",
        (2, 3) => "f",
        (2, 4) => "g",
        (6, 5) => "h",
        (6, 6) => "j",
        (6, 7) => "k",
        (6, 8) => "l",
        (6, 9) => "BS",
        (3, 0) => "!",
        (3, 1) => "z",
        (3, 2) => "x",
        (3, 3) => "c",
        (3, 4) => "v",
        (7, 5) => "b",
        (7, 6) => "n",
        (7, 7) => "m",
        (7, 8) => "?",
        //(7, 9) => "↩️",
        (7, 9) => "RET",

        (8, 5) => "LS",
        (8, 6) => ",",
        (8, 7) => "SP",
        (8, 8) => ".",
        (8, 9) => "RS",
        (8, 0) => "F1",
        (8, 1) => "F2",
        (3, 8) => "F3",
        (3, 9) => "F4",
        (8, 3) => "←",
        //(8, 3) => "LT",
        (3, 6) => "→",
        //(3, 6) => "RT",
        (6, 4) => "↑",
        //(6, 4) => "UP",
        (8, 2) => "↓",
        //(8, 2) => "DN",
        (5, 2) => "MID",
        _ => "ERR!",
    }
}

fn render_string(txt: &mut String, remaining: &Vec::<(RowCol, bool)>, time_remaining: u64) {
    txt.clear();
    let mut keyrowstrs: [String; 7] = [
        String::from("▶ "),
        String::from("▶ "),
        String::from("▶ "),
        String::from("▶ "),
        String::from("▶ "),
        String::from("▶ "),
        String::from("▶ "),
    ];
    for &(code, was_hit) in remaining.iter() {
        if !was_hit {
            // lookup table to help organize the key hits remaining
            let draw_row = match code.r {
                0 | 4 => 2,
                1 => 3,
                5 => if code.c != 2 {3} else {0},
                2 => 4,
                6 => if code.c != 4 {4} else {0},
                3 => if code.c <= 4 {5} else { if code.c == 6 {0} else {1} }
                7 => 5,
                8 => if code.c >= 5 {6} else { if code.c <= 1 {1} else {0} },
                _ => 6,
            };
            keyrowstrs[draw_row].push_str(map_codes(code));
            keyrowstrs[draw_row].push_str("\t");
        }
    }
    for s in keyrowstrs.iter() {
        if s.chars().count() > 2 {
            txt.push_str(s);
            txt.push('\n');
        } else {
            txt.push_str("✔\n")
        }
    }
    txt.push_str("Timeout: ");
    txt.push_str(&(time_remaining / 1000).to_string());
    txt.push_str("s");
    //txt.push_str("\n\n\n\n");
}

fn ping_thread(conn: usize, timeout: usize) {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let start = tt.elapsed_ms();
    let mut krs_ser: [(u8, u8); 32] = [(255, 255); 32];
    krs_ser[0] = (254, 254);
    while tt.elapsed_ms() - start < timeout as u64 {
        tt.sleep_ms(2000).unwrap();
        let buf = xous_ipc::Buffer::into_buf(krs_ser).or(Err(xous::Error::InternalError)).expect("couldn't serialize krs buffer");
        buf.send(conn as xous::CID, OqcOp::KeyCode.to_u32().unwrap()).expect("couldn't send raw scancodes");
    }
}