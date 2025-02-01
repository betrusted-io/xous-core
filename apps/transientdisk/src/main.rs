#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use core::fmt::Write;

use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use num_traits::*;
mod flash_drive;

pub(crate) const SERVER_NAME_TRANSIENTDISK: &str = "_Transient Disk_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum TradOp {
    /// Redraw the screen
    Redraw = 0,

    FocusChange,

    Read,
    Write,
    MaxLba,

    /// Quit the application
    Quit,
}

// Framework code to draw a string of text on the screen.
// Taken from hello app.
struct UI {
    content: Gid,
    gam: gam::Gam,
    _gam_token: [u32; 4],
    screensize: Point,
    #[cfg(feature = "tts")]
    tts: TtsFrontend,
}

impl UI {
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(&xns).expect("Can't connect to GAM");
        let gam_token = gam
            .register_ux(gam::UxRegistration {
                app_name: String::from(gam::APP_NAME_TRANSIENTDISK),
                ux_type: gam::UxType::Chat,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: TradOp::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: None,
                focuschange_id: Some(TradOp::FocusChange.to_u32().unwrap()),
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
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    self.screensize,
                    DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
                ),
            )
            .expect("can't clear content area");
    }

    /// Redraw the text view onto the screen.
    fn redraw(&mut self) {
        self.clear_area();

        let mut text_view = TextView::new(
            self.content,
            TextBounds::GrowableFromBr(
                Point::new(self.screensize.x - 45, self.screensize.y - (self.screensize.y / 2)),
                (self.screensize.x / 5 * 4) as u16,
            ),
        );

        text_view.border_width = 1;
        text_view.draw_border = true;
        text_view.clear_area = true;
        text_view.rounded_border = Some(3);
        text_view.style = GlyphStyle::ExtraLarge;
        write!(text_view.text, "1.44mb flash drive now available.").expect("Could not write to text view");

        self.gam.post_textview(&mut text_view).expect("Could not render text view");
        self.gam.redraw().expect("Could not redraw screen");
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Hello world PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_TRANSIENTDISK, None).expect("can't register server");

    #[cfg(not(feature = "mass-storage"))]
    log::warn!(
        "transientdisk has been compiled without the mass-storage feature, needed to make the USB subsystem behave like a mass-storage device."
    );

    let mut ui = UI::new(&xns, sid);

    let mut fd = flash_drive::FlashDrive::new(1445888, 512).expect("cannot create flash drive instance");

    #[cfg(feature = "mass-storage")]
    let usb = usb_device_xous::UsbHid::new();

    #[cfg(feature = "mass-storage")]
    let core_before_ms = usb.get_current_core().unwrap();

    let mut usb_setup = false;

    loop {
        let mut msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(TradOp::Redraw) => {
                log::info!("Got redraw");
                ui.redraw();
            }
            Some(TradOp::Quit) => {
                log::info!("Quitting application");
                break;
            }
            Some(TradOp::FocusChange) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        #[cfg(feature = "mass-storage")]
                        {
                            usb.reset_block_device();
                            usb.switch_to_core(core_before_ms).unwrap();
                        }
                    }
                    gam::FocusState::Foreground => {
                        if usb_setup {
                            continue;
                        }

                        #[cfg(feature = "mass-storage")]
                        {
                            usb.set_block_device(
                                TradOp::Read.to_usize().unwrap(),
                                TradOp::Write.to_usize().unwrap(),
                                TradOp::MaxLba.to_usize().unwrap(),
                            );

                            usb.set_block_device_sid(sid.clone());

                            usb.switch_to_core(usb_device_xous::UsbDeviceType::MassStorage).unwrap();
                        }
                        usb_setup = true;
                    }
                }
            }),
            Some(TradOp::Read) => {
                fd.read(&mut msg);
            }
            Some(TradOp::Write) => {
                fd.write(&mut msg);
            }
            Some(TradOp::MaxLba) => fd.max_lba(&mut msg),
            _ => {
                log::error!("Got unknown message");
            }
        }
    }

    log::info!("Quitting");
    xous::terminate_process(0)
}
