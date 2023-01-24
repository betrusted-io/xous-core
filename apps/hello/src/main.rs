#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use core::fmt::Write;
use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use num_traits::*;
use locales::t;
#[cfg(feature = "tts")]
use tts_frontend::*;

/// Basic 'Hello World!' application that draws a simple
/// TextView to the screen.

pub(crate) const SERVER_NAME_HELLO: &str = "_Hello World_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum HelloOp {
    /// Redraw the screen
    Redraw = 0,

    FocusChange,

    Read,
    Write,
    MaxLba,

    /// Quit the application
    Quit,
}

struct Hello {
    content: Gid,
    gam: gam::Gam,
    _gam_token: [u32; 4],
    screensize: Point,
    #[cfg(feature = "tts")]
    tts: TtsFrontend,
}

impl Hello {
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(&xns).expect("Can't connect to GAM");
        let gam_token = gam
            .register_ux(gam::UxRegistration {
                app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_HELLO),
                ux_type: gam::UxType::Chat,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: HelloOp::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: None,
                focuschange_id: Some(HelloOp::FocusChange.to_u32().unwrap()),
            })
            .expect("Could not register GAM UX")
            .unwrap();

        let content = gam
            .request_content_canvas(gam_token)
            .expect("Could not get content canvas");
        let screensize = gam
            .get_canvas_bounds(content)
            .expect("Could not get canvas dimensions");
        Self {
            gam,
            _gam_token: gam_token,
            content,
            screensize,
            #[cfg(feature = "tts")]
            tts: TtsFrontend::new(xns).unwrap(),
        }
    }

    /// Clear the entire screen.
    fn clear_area(&self) {
        self.gam
            .draw_rectangle(
                self.content,
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    self.screensize,
                    DrawStyle {
                        fill_color: Some(PixelColor::Light),
                        stroke_color: None,
                        stroke_width: 0,
                    },
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
                Point::new(
                    self.screensize.x - (self.screensize.x / 2),
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
        write!(text_view.text, "{}", t!("helloworld.hello", xous::LANG)).expect("Could not write to text view");
        #[cfg(feature="tts")]
        self.tts.tts_simple(t!("helloworld.hello", xous::LANG)).unwrap();

        self.gam
            .post_textview(&mut text_view)
            .expect("Could not render text view");
        self.gam.redraw().expect("Could not redraw screen");
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Hello world PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns
        .register_name(SERVER_NAME_HELLO, None)
        .expect("can't register server");

    let mut hello = Hello::new(&xns, sid);

    let conn = xous::connect(sid).unwrap();

    const CAPACITY: usize = 256 * 1024; // must be a multiple of one page (4096)
    let mut backing = xous::syscall::map_memory(
        None,
        None,
        CAPACITY,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ).unwrap();
    {
        let backing_slice: &mut [u32] = backing.as_slice_mut();
        for (index, d) in backing_slice.iter_mut().enumerate() {
            *d = index as u32;
        }
    }

    let mut usb_setup = false;
    loop {
        let mut msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(HelloOp::Redraw) => {
                log::info!("Got redraw");
                hello.redraw();
            }
            Some(HelloOp::Quit) => {
                log::info!("Quitting application");
                break;
            }
            Some(HelloOp::FocusChange) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                if usb_setup {
                    continue;
                }
                
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                    }
                    gam::FocusState::Foreground => {
                        let usb = usb_device_xous::UsbHid::new();

                        usb.set_block_device(
                            HelloOp::Read.to_usize().unwrap(), 
                            HelloOp::Write.to_usize().unwrap(),
                            HelloOp::MaxLba.to_usize().unwrap(),
                        );

                        usb.set_block_device_sid(sid.clone());

                        usb.switch_to_core(usb_device_xous::UsbDeviceType::MassStorage).unwrap();
                        usb_setup = true;
                    }
                }
            }),
            Some(HelloOp::Read) => {
                let body = msg.body.memory_message_mut().expect("incorrect message type received");
                let lba = body.offset.map(|v| v.get()).unwrap_or_default();
                let data = body.buf.as_slice_mut::<u8>();
                let block_bytes: usize = 512;

                let backing_slice: &[u8] = backing.as_slice();

                let rawdata = &backing_slice[lba as usize * block_bytes..(lba as usize + 1) * block_bytes];

                data[..block_bytes].copy_from_slice(
                    rawdata
                );
            },
            Some(HelloOp::Write) => {
                let body = msg.body.memory_message_mut().expect("incorrect message type received");
                let lba = body.offset.map(|v| v.get()).unwrap_or_default();
                let data = body.buf.as_slice_mut::<u8>();

                let block_bytes: usize = 512;

                let backing_slice: &mut [u8] = backing.as_slice_mut();
                backing_slice[lba as usize * block_bytes..(lba as usize + 1) * block_bytes].copy_from_slice(&data[..block_bytes]);
            },
            Some(HelloOp::MaxLba) => {
                xous::return_scalar(msg.sender, (CAPACITY / 512) - 1).unwrap();
            },
            _ => {
                log::error!("Got unknown message");
            }
        }
    }

    log::info!("Quitting");
    xous::terminate_process(0)
}
