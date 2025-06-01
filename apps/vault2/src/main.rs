use core::fmt::Write;

use blitstr2::GlyphStyle;
use locales::t;
use num_traits::*;
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// Redraw the screen
    Redraw = 0,

    /// Quit the application
    Quit,
}

struct VaultUi {
    pub gfx: Gfx,
}

impl VaultUi {
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self { Self { gfx: Gfx::new(&xns).unwrap() } }

    /// Clear the entire screen.
    fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Redraw the text view onto the screen.
    fn redraw(&mut self) {
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(Point::new(0, 0), Point::new(127, 12))),
        );
        write!(tv, "hello world").ok();
        self.gfx.draw_textview(&mut tv).expect("couldn't draw text");
        self.gfx.flush().ok();
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Vault2 PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_VAULT2, None).expect("can't register server");

    let mut vault_ui = VaultUi::new(&xns, sid);

    std::thread::spawn({
        let sid = sid.clone();
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let cid_to_self = xous::connect(sid).unwrap();
            loop {
                tt.sleep_ms(500).ok();
                log::info!("ping");
                xous::send_message(
                    cid_to_self,
                    xous::Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't pump the main loop event thread");
            }
        }
    });

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
                log::debug!("Got redraw");
                vault_ui.redraw();
            }
            Some(VaultOp::Quit) => {
                log::info!("Quitting application");
                break;
            }
            _ => {
                log::error!("Got unknown message");
            }
        }
    }

    log::info!("Quitting");
    xous::terminate_process(0)
}
