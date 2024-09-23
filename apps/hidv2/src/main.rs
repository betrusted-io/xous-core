#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use core::fmt::Write;
use std::sync::mpsc::{self, Receiver, Sender};

use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use num_traits::*;
use usb_device_xous::UsbHid;

pub(crate) const SERVER_NAME_HIDV2: &str = "_HIDv2_";

// stealing fido report for the lulz
const FIDO_REPORT_DESCRIPTOR: &[u8] = &[
    0x06, 0xD0, 0xF1, // Usage Page (FIDO),
    0x09, 0x01, // Usage (U2F Authenticator Device)
    0xA1, 0x01, // Collection (Application),
    0x09, 0x20, //   Usage (Data In),
    0x15, 0x00, //       Logical Minimum(0),
    0x26, 0xFF, 0x00, // Logical Maxs (0x00FF),
    0x75, 0x08, //       Report size (8)
    0x95, 0x40, //       Report count (64)
    0x81, 0x02, //       Input (Data | Variable | Absolute)
    0x09, 0x21, //   Usage (Data Out),
    0x15, 0x00, //       Logical Minimum(0),
    0x26, 0xFF, 0x00, // Logical Maxs (0x00FF),
    0x75, 0x08, //       Report size (8)
    0x95, 0x40, //       Report count (64)
    0x91, 0x02, //       Output (Data | Variable | Absolute)
    0xC0, // End Collection
];

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum HIDv2Op {
    /// Redraw the screen
    Redraw = 0,

    /// Enter/exit app
    FocusChange,

    /// Quit the application
    Quit,
}

struct HIDv2Demo {
    content: Gid,
    gam: gam::Gam,
    _gam_token: [u32; 4],
    screensize: Point,
}

impl HIDv2Demo {
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(&xns).expect("Can't connect to GAM");
        let gam_token = gam
            .register_ux(gam::UxRegistration {
                app_name: String::from(gam::APP_NAME_HIDV2),
                ux_type: gam::UxType::Chat,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: HIDv2Op::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: None,
                focuschange_id: Some(HIDv2Op::FocusChange.to_u32().unwrap()),
            })
            .expect("Could not register GAM UX")
            .unwrap();

        let content = gam.request_content_canvas(gam_token).expect("Could not get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("Could not get canvas dimensions");
        Self { gam, _gam_token: gam_token, content, screensize }
    }

    /// Clear the entire screen.
    fn clear_area(&self) {
        self.gam
            .draw_rectangle(
                self.content,
                Rectangle::new_with_style(Point::new(0, 0), self.screensize, DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0,
                }),
            )
            .expect("can't clear content area");
    }

    /// Redraw the text view onto the screen.
    fn redraw(&mut self) {
        self.clear_area();

        let mut text_view = TextView::new(
            self.content,
            TextBounds::GrowableFromBr(
                Point::new(
                    self.screensize.x - (self.screensize.x / 2) + 40,
                    self.screensize.y - (self.screensize.y / 2),
                ),
                (self.screensize.x / 5 * 4) as u16,
            ),
        );

        text_view.border_width = 1;
        text_view.draw_border = true;
        text_view.clear_area = true;
        text_view.rounded_border = Some(3);
        text_view.style = GlyphStyle::Regular;
        write!(text_view.text, "{}", "Echoing whatever on HID USB stack...")
            .expect("Could not write to text view");

        self.gam.post_textview(&mut text_view).expect("Could not render text view");
        self.gam.redraw().expect("Could not redraw screen");
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("HIDv2 Demo PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    let mut allow_redraw = true;

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_HIDV2, None).expect("can't register server");

    let mut hidv2_demo = HIDv2Demo::new(&xns, sid);

    let mut hid_thread_handle: Option<Sender<bool>> = None;

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(HIDv2Op::Redraw) => {
                log::debug!("Got redraw");

                if !allow_redraw {
                    continue;
                }

                hidv2_demo.redraw();
            }
            Some(HIDv2Op::Quit) => {
                log::info!("Quitting application");
                break;
            }
            Some(HIDv2Op::FocusChange) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                log::info!("focus change: {:?}", new_state);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false; // this instantly terminates future updates, even if Pump messages are in our input queue
                        match hid_thread_handle {
                            Some(handle) => {
                                handle.send(true).unwrap();
                                hid_thread_handle = None;
                            }
                            None => (),
                        }
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                        hid_thread_handle = Some(start_hid_thread())
                    }
                }
            }),
            _ => {
                log::error!("Got unknown message");
            }
        }
    }

    log::info!("Quitting");
    xous::terminate_process(0)
}

fn start_hid_thread() -> Sender<bool> {
    let (tx, rx): (Sender<bool>, Receiver<bool>) = mpsc::channel();

    let usbd = UsbHid::new();

    usbd.connect_hid_app(FIDO_REPORT_DESCRIPTOR.to_vec()).unwrap();

    usbd.switch_to_core(usb_device_xous::UsbDeviceType::HIDv2).unwrap();

    std::thread::spawn(move || {
        loop {
            match rx.try_recv() {
                Ok(_) => break,
                Err(_) => {
                    match usbd.read_report() {
                        Err(err) => match err {
                            xous::Error::UnknownError => (),
                            _ => log::error!("error while reading report! {:#?}", err),
                        },
                        Ok(report) => {
                            usbd.write_report(report).unwrap();
                        }
                    };
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        usbd.switch_to_core(usb_device_xous::UsbDeviceType::Debug).unwrap();
    });

    return tx;
}
