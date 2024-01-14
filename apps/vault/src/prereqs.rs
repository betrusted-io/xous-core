use std::fmt::Write;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc};
use std::thread;
use std::time::SystemTime;

use chrono::{TimeZone, Utc};
use gam::{GlyphStyle, UxRegistration};
use graphics_server::{DrawStyle, PixelColor, Point, Rectangle, TextView};
use locales::t;
use num_traits::*;
use sntpc::{Error, NtpContext, NtpTimestampGenerator, NtpUdpSocket, Result};
use xous::{send_message, Message};

use crate::VaultOp;

pub(crate) fn prereqs(sid: xous::SID, time_conn: xous::CID) -> ([u32; 4], bool) {
    let xns = xous_names::XousNames::new().unwrap();
    let pddb = pddb::PddbMountPoller::new();
    let gam = gam::Gam::new(&xns).expect("can't connect to GAM");

    let app_name_ref = gam::APP_NAME_VAULT;
    let token = gam
        .register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(app_name_ref),
            ux_type: gam::UxType::Chat,
            predictor: Some(xous_ipc::String::<64>::from_str(crate::ux::icontray::SERVER_NAME_ICONTRAY)),
            listener: sid.to_array(), /* note disclosure of our SID to the GAM -- the secret is now shared
                                       * with the GAM! */
            redraw_id: VaultOp::Redraw.to_u32().unwrap(),
            gotinput_id: Some(VaultOp::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
            focuschange_id: Some(VaultOp::ChangeFocus.to_u32().unwrap()),
        })
        .expect("couldn't register Ux context for repl")
        .unwrap();

    let content = gam.request_content_canvas(token).expect("couldn't get content canvas");
    let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
    gam.toggle_menu_mode(token).expect("couldnt't toggle menu mode");

    let self_conn = xous::connect(sid).unwrap();
    let run_pump = Arc::new(AtomicBool::new(true));
    let _ = thread::spawn({
        let run_pump = run_pump.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            while run_pump.load(Ordering::SeqCst) {
                tt.sleep_ms(1120).unwrap();
                send_message(self_conn, Message::new_scalar(VaultOp::Nop.to_usize().unwrap(), 0, 0, 0, 0))
                    .ok();
            }
        }
    });

    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        let opcode: Option<VaultOp> = FromPrimitive::from_usize(msg.body.id());
        log::trace!("{:?}", opcode);
        let is_mounted = pddb.is_mounted_nonblocking();
        let time_init = match send_message(
            time_conn,
            Message::new_blocking_scalar(6 /* WallClockTimeInit */, 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(init)) => {
                if init == 1 {
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_mounted && time_init {
            // this will send a message to the future receiver of the server message
            send_message(self_conn, Message::new_scalar(VaultOp::FullRedraw.to_usize().unwrap(), 0, 0, 0, 0))
                .ok();
            break;
        }
        match opcode {
            Some(VaultOp::Redraw) | Some(VaultOp::FullRedraw) => {
                if allow_redraw {
                    gam.draw_rectangle(
                        content,
                        Rectangle::new_with_style(
                            Point::new(0, 0),
                            screensize,
                            DrawStyle {
                                fill_color: Some(PixelColor::Light),
                                stroke_color: None,
                                stroke_width: 0,
                            },
                        ),
                    )
                    .expect("can't clear content area");

                    let mut title_text = TextView::new(
                        content,
                        graphics_server::TextBounds::CenteredBot(Rectangle::new(
                            Point::new(0, 0),
                            Point::new(screensize.x, 150),
                        )),
                    );
                    title_text.draw_border = false;
                    title_text.clear_area = true;
                    title_text.style = GlyphStyle::Bold;
                    if !is_mounted {
                        write!(title_text, "{}\n\n", t!("vault.error.mount_pddb", locales::LANG)).ok();
                    }
                    if !time_init {
                        write!(title_text, "{}", t!("vault.error.time_init", locales::LANG)).ok();
                    }
                    gam.post_textview(&mut title_text).expect("couldn't post title");
                }
            }
            Some(VaultOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                    }
                }
            }),
            _ => {} // ignore unrecognized opcodes
        }
    }
    run_pump.store(false, Ordering::SeqCst);
    (token, allow_redraw)
}

#[derive(Debug)]
struct UdpSocketWrapper(UdpSocket);

impl NtpUdpSocket for UdpSocketWrapper {
    fn send_to<T: ToSocketAddrs>(&self, buf: &[u8], addr: T) -> Result<usize> {
        match self.0.send_to(buf, addr) {
            Ok(usize) => Ok(usize),
            Err(_) => Err(Error::Network),
        }
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        match self.0.recv_from(buf) {
            Ok((size, addr)) => Ok((size, addr)),
            Err(_) => Err(Error::Network),
        }
    }
}

#[derive(Copy, Clone, Default)]
struct StdTimestampGen {
    duration: std::time::Duration,
}
impl NtpTimestampGenerator for StdTimestampGen {
    fn init(&mut self) {
        self.duration =
            std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();
    }

    fn timestamp_sec(&self) -> u64 { self.duration.as_secs() }

    fn timestamp_subsec_micros(&self) -> u32 { self.duration.subsec_micros() }
}

pub(crate) fn ntp_updater(time_conn: xous::CID) {
    let _ = thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let trng = trng::Trng::new(&xns).unwrap();
            let netmgr = net::NetManager::new();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let mut now = SystemTime::now();
            let mut force_update = true;
            tt.sleep_ms(1000 * 60 * 2).ok(); // initial delay of 2 minutes before polling. This gives plenty of time for network to come up.
            loop {
                if force_update || now.elapsed().unwrap().as_secs() > 3600 * 24 {
                    // once a day in real time
                    // check if we have a network connection. if not, repeat the loop, after a short delay
                    match netmgr.get_ipv4_config() {
                        Some(conf) => {
                            if conf.dhcp != com_rs::DhcpState::Bound {
                                log::debug!("no DHCP");
                                tt.sleep_ms(1000 * 31).unwrap();
                                continue;
                            }
                        }
                        None => {
                            log::debug!("no network connection");
                            tt.sleep_ms(1000 * 43).unwrap();
                            continue;
                        }
                    }
                    // get time from NTP
                    let local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
                    let socket_addr = SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
                        local_port,
                    );
                    let socket = UdpSocket::bind(socket_addr).expect("Unable to create UDP socket");
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .expect("Unable to set UDP socket read timeout");
                    let sock_wrapper = UdpSocketWrapper(socket);
                    let ntp_context = NtpContext::new(StdTimestampGen::default());
                    let result = sntpc::get_time("time.google.com:123", sock_wrapper, ntp_context);
                    match result {
                        Ok(time) => {
                            log::debug!("Got NTP time: {}.{}", time.sec(), time.sec_fraction());
                            let current_time = Utc.ymd(1970, 1, 1).and_hms(0, 0, 0)
                                + chrono::Duration::seconds(time.sec() as i64);
                            log::debug!("Setting UTC time: {:?}", current_time.to_string());
                            xous::send_message(
                                time_conn,
                                Message::new_scalar(
                                    2, /* SetUtcTimeMs */
                                    ((current_time.timestamp_millis() as u64) >> 32) as usize,
                                    (current_time.timestamp_millis() as u64 & 0xFFFF_FFFF) as usize,
                                    0,
                                    0,
                                ),
                            )
                            .expect("couldn't set time");
                            now = SystemTime::now();
                            force_update = false;
                        }
                        Err(err) => {
                            // if NTP server is down, wait a bit longer and try again
                            log::warn!("NTP failed, err: {:?}", err);
                            tt.sleep_ms(1000 * 127).unwrap();
                        }
                    }
                } else {
                    tt.sleep_ms(1000 * 60 * 3).unwrap(); // once every 3 minutes of screen-on time, poll the loop.
                }
            }
        }
    });
}
