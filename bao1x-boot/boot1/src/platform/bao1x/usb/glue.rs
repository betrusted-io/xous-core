use alloc::boxed::Box;
use core::cell::RefCell;
use core::convert::TryInto;

use bao1x_api::baosec::SPINOR_ERASE_SIZE;
use bao1x_api::*;
use bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR;
use bao1x_hal::ifram::IframRange;
use bao1x_hal::iox::Iox;
use bao1x_hal::udma::*;
use bao1x_hal::usb::driver::UsbDeviceState;
use critical_section::Mutex;
use utralib::*;

use crate::platform::usb::page_defrag::PageAssembler;
use crate::uf2::Uf2Block;
use crate::usb;

pub(crate) static SECTOR: Mutex<RefCell<Uf2Sector>> = Mutex::new(RefCell::new(Uf2Sector::new(0)));
pub type PageCallback = fn(usize, Box<[u8; SPINOR_ERASE_SIZE as usize]>);
pub static SECTOR_TRACKER: Mutex<RefCell<Option<PageAssembler<PageCallback>>>> =
    Mutex::new(RefCell::new(None));

// Empirically measured PORTSC when the port is unplugged. This might be a brittle way
// to detect if the device is unplugged.
const DISCONNECT_STATE: u32 = 0x40b; //  01_0_0000_0_1_01_1
const DISCONNECT_STATE_HS: u32 = 0xc6b; // 11_0_0011_0_1_01_1

pub fn is_disconnected(state: u32) -> bool { state == DISCONNECT_STATE_HS || state == DISCONNECT_STATE }

pub fn setup_spim(perclk: u32) {
    critical_section::with(|cs| SECTOR_TRACKER.borrow(cs).replace(Some(PageAssembler::new(write_spim_page))));

    let udma_global = GlobalConfig::new();

    // setup the I/O pins
    let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
    let channel = bao1x_hal::board::setup_memory_pins(&iox);
    udma_global.clock_on(PeriphId::from(channel));
    // safety: this is safe because clocks have been set up
    let mut flash_spim = unsafe {
        Spim::new_with_ifram(
            channel,
            // has to be half the clock frequency reaching the block, but
            // run it as fast
            // as we can run perclk
            perclk / 4,
            perclk / 2,
            SpimClkPol::LeadingEdgeRise,
            SpimClkPha::CaptureOnLeading,
            SpimCs::Cs0,
            0,
            0,
            None,
            256 + 16, /* just enough space to send commands + programming
                       * page */
            4096,
            Some(6),
            None,
            IframRange::from_raw_parts(SPIM_FLASH_IFRAM_ADDR, SPIM_FLASH_IFRAM_ADDR, 4096 * 2),
        )
    };
    flash_spim.mem_qpi_mode(false);
    let flash_id = flash_spim.mem_read_id_flash();
    crate::println_d!("flash ID (init): {:x}", flash_id);

    flash_spim.mem_qpi_mode(true);
    let flash_id = flash_spim.mem_read_id_flash();
    crate::println_d!("QPI flash ID: {:x}", flash_id);
}

pub fn write_spim_page(addr: usize, data: Box<[u8; SPINOR_ERASE_SIZE as usize]>) {
    // conjure a handle to SPIM. The function has to work through a Mutex and the structure below
    // isn't safe to pass through the Mutex, so we re-create the handle here without re-initializing the
    // hardware itself. This is only safe because we're sure there are no other concurrent calls trying
    // to do this in this environment.
    let mut flash_spim = unsafe {
        Spim::from_raw_parts(
            match bao1x_hal::board::SPI_MEM_CHANNEL {
                SpimChannel::Channel0 => utra::udma_spim_0::HW_UDMA_SPIM_0_BASE,
                SpimChannel::Channel1 => utra::udma_spim_1::HW_UDMA_SPIM_1_BASE,
                SpimChannel::Channel2 => utra::udma_spim_2::HW_UDMA_SPIM_2_BASE,
                SpimChannel::Channel3 => utra::udma_spim_3::HW_UDMA_SPIM_3_BASE,
            },
            SpimCs::Cs0,
            0,
            0,
            None,
            SpimMode::Quad,
            SpimByteAlign::Disable,
            IframRange::from_raw_parts(SPIM_FLASH_IFRAM_ADDR, SPIM_FLASH_IFRAM_ADDR, 4096 * 2),
            256 + 16,
            4096,
            6,
        )
    };
    crate::println_d!("Erasing sector at addr {:x}", addr);
    flash_spim.flash_erase_sector(addr as u32);

    for (page_offset, buf) in data.chunks_exact(256).enumerate() {
        // crate::println_d!("writing page at {:x}: {:x?}", addr + page_offset * FLASH_PAGE_LEN, &buf[..8]);
        flash_spim
            .mem_flash_write_page((addr + page_offset * FLASH_PAGE_LEN) as u32, buf.try_into().unwrap());
    }
}

pub fn setup() -> (UsbDeviceState, u32) {
    crate::println!(
        "RAM disk starts at {:x} and advertises {}kiB in length, but is actually {}kiB of storage",
        usb::RAMDISK_ADDRESS,
        usb::RAMDISK_LEN / 1024,
        usb::RAMDISK_ACTUAL_LEN / 1024,
    );

    // safety: this is safe because we're calling this before any access to `USB` static mut
    // state, and we also understand that the .data section doesn't exist in the loader and
    // we've taken countermeasures to initialize everything "from code", i.e. not relying
    // on static compile-time assignments for the static mut state.
    unsafe { crate::platform::bao1x::usb::init_usb() };

    // Below is all unsafe because USB is global mutable state
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            usb.reset();
            usb.init();
            usb.start();
            usb.update_current_speed();
            // IRQ enable must happen without dependency on the hardware lock
            usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
            usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);

            let last_usb_state = usb.get_device_state();
            let portsc = usb.portsc_val();
            crate::println_d!("USB state: {:?}, {:x}", last_usb_state, portsc);
            (last_usb_state, portsc)
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub fn shutdown() {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            crate::irq::disable_all_irqs();
            crate::println!("Stopping USB...");
            usb.stop();
        }
    }
}

pub fn usb_status() -> (UsbDeviceState, u32) {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            (usb.get_device_state(), usb.portsc_val())
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub fn flush_tx() {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::bao1x::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            crate::usb::handlers::flush_tx(usb);
        } else {
            panic!("USB core not allocated, can't proceed!");
        }
    }
}

pub struct Uf2Sector {
    pub address: usize,
    pub data: [u8; 512],
    // pointer that tracks how far we've written into the `data` array
    pub progress: usize,
}

impl Uf2Sector {
    pub const fn new(address: usize) -> Self { Self { address, data: [0u8; 512], progress: 0 } }

    /// Takes in a slice of incoming data, and a notional "disk address" into which it should be writing
    pub fn extend_from_slice(&mut self, address: usize, slice: &[u8]) -> (Option<Self>, Option<Uf2Block>) {
        if address != self.address + self.progress {
            /*
            crate::println!(
                "Resetting sector address tracker, expected {:x} got {:x}",
                self.address + self.progress,
                address
            );
            */
            self.address = address % 512;
        }

        let copylen = (self.data.len() - self.progress).min(slice.len());
        for (dst, &src) in self.data[self.progress..].iter_mut().zip(slice.iter()) {
            *dst = src;
        }
        self.progress += copylen;

        // note that the Uf2Block::from_bytes() function gracefully fails and returns None
        // in the case that the user wrote non-uf2 data to our "disk"
        let decoded = if self.progress >= self.data.len() { Uf2Block::from_bytes(&self.data) } else { None };

        let new_sector = if copylen < slice.len() {
            // handle the case that we had too much data for this sector
            let mut sector = Uf2Sector::new(self.address + self.data.len());
            for (dst, &src) in sector.data.iter_mut().zip(&slice[copylen..]) {
                *dst = src;
            }
            sector.progress = slice.len() - copylen;
            Some(sector)
        } else {
            if copylen == slice.len() && self.progress >= self.data.len() {
                // handles the case that we just finished the sector nicely
                Some(Uf2Sector::new(self.address + self.data.len()))
            } else {
                None
            }
        };

        (new_sector, decoded)
    }
}
