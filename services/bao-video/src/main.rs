use cramium_hal::iox::{IoxPort, IoxValue};
use cramium_hal::sh1107::{Mono, Oled128x128};
use cramium_hal::udma::PeriphId;
use num_traits::ToPrimitive;
use utralib::utra;
use ux_api::minigfx::*;

mod gfx;
mod homography;
mod modules;
mod qr;
mod api;
use api::*;
#[cfg(feature = "board-baosec")]
mod hw;
#[cfg(feature = "board-baosec")]
use hw::wrapped_main;

#[cfg(feature = "hosted-baosec")]
mod hosted;
#[cfg(feature = "hosted-baosec")]
use hosted::wrapped_main;

//! Scope of this crate:
//!
//! bao-video contains the platform-specific drivers for the baosec platform that pertain
//! to video: both the capture of video, as well as any operations involving drawing to
//! the display (rendering graphics primitives, etc).
//!
//! Note that explicitly out of scope are the higher-level API calls for UI management, e.g.
//! creation of modals and managing draw lists. Only the hardware renderers should be implemented
//! in this crate. Think of it like a kernel module that handles a video subsystem, where both
//! camera and display are co-located in the same module for fast data sharing (keep in mind
//! this is a microkernel, so we don't have a monolith data space like Linux: all drivers are
//! in their own process space unless explicitly co-located).
//!
//! It also pulls in QR code processing for performance reasons - by keeping the QR code
//! processing in the process space of the camera, we can avoid an expensive memcopy between
//! process spaces and improve the responsiveness of the feedback loop while QR searching happens.

pub const IMAGE_WIDTH: usize = 256;
pub const IMAGE_HEIGHT: usize = 240;
pub const BW_THRESH: u8 = 128;

// Next steps for performance improvement:
//
// Improve qr::mapping -> point_from_hv_lines such that we're not just deriving the HV
// lines from the the edges of the finder regions, we're also using the very edge of
// the whole QR code itself to guide the line. This will improve the intersection point
// so that we can accurately hit the "fourth corner". At the moment it's sort of a
// luck of the draw if the interpolation hits exactly right, or if we're roughly a module
// off from ideal, which causes the data around that point to be interpreted incorrectly.

fn main() -> ! {
    let stack_size = 1 * 1024 * 1024;
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}
