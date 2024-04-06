use core::mem::size_of;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, KeyInit, Nonce, Tag};
use cramium_hal::ifram::IframRange;
use cramium_hal::iox::*;
use cramium_hal::udma::*;

use crate::bootconfig::BootConfig;
use crate::platform::{SPIM_FLASH_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR};
use crate::println;
use crate::swap::*;

/// hard coded at offset 0 of SPI FLASH for now, until we figure out if and how to move this around.
const SWAP_IMG_START: usize = 0;

pub struct SwapHal {
    image_start: usize,
    image_mac_start: usize,
    partial_nonce: [u8; 8],
    // overflow AAD with panic if it's longer than this!
    aad_storage: [u8; 64],
    aad_len: usize,
    cipher: Aes256GcmSiv,
    flash_spim: Spim,
    ram_spim: Spim,
    iox: Iox,
    udma_global: GlobalConfig,
    buf: RawPage,
}

fn setup_port(
    iox: &mut Iox,
    port: IoxPort,
    pin: u8,
    function: Option<IoxFunction>,
    direction: Option<IoxDir>,
    drive: Option<IoxDriveStrength>,
    slow_slew: Option<IoxEnable>,
    schmitt: Option<IoxEnable>,
    pullup: Option<IoxEnable>,
) {
    if let Some(f) = function {
        iox.set_alternate_function(port, pin, f);
    }
    if let Some(d) = direction {
        iox.set_gpio_dir(port, pin, d);
    }
    if let Some(t) = schmitt {
        iox.set_gpio_schmitt_trigger(port, pin, t);
    }
    if let Some(p) = pullup {
        iox.set_gpio_pullup(port, pin, p);
    }
    if let Some(s) = slow_slew {
        iox.set_slow_slew_rate(port, pin, s);
    }
    if let Some(s) = drive {
        iox.set_drive_strength(port, pin, s);
    }
}

impl SwapHal {
    pub fn new(cfg: &BootConfig) -> Option<SwapHal> {
        if let Some(swap) = cfg.swap {
            // sanity check this structure
            assert_eq!(core::mem::size_of::<SwapSourceHeader>(), 4096);

            // setup the I/O pins
            let mut iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
            // JQSPI1
            // SPIM_CLK_A[0]
            setup_port(
                &mut iox,
                IoxPort::PD,
                4,
                Some(IoxFunction::AF1),
                Some(IoxDir::Output),
                Some(IoxDriveStrength::Drive2mA),
                Some(IoxEnable::Disable),
                None,
                None,
            );
            // SPIM_SD[0-3]_A[0]
            for i in 0..3 {
                setup_port(
                    &mut iox,
                    IoxPort::PD,
                    i,
                    Some(IoxFunction::AF1),
                    None,
                    Some(IoxDriveStrength::Drive2mA),
                    Some(IoxEnable::Enable),
                    None,
                    None,
                );
            }
            // SPIM_CSN0_A[0]
            setup_port(
                &mut iox,
                IoxPort::PD,
                5,
                Some(IoxFunction::AF1),
                Some(IoxDir::Output),
                Some(IoxDriveStrength::Drive2mA),
                Some(IoxEnable::Enable),
                None,
                None,
            );
            // SPIM_CSN0_A[1]
            setup_port(
                &mut iox,
                IoxPort::PD,
                6,
                Some(IoxFunction::AF1),
                Some(IoxDir::Output),
                Some(IoxDriveStrength::Drive2mA),
                Some(IoxEnable::Enable),
                None,
                None,
            );
            /* // JPC7_13
            // SPIM_CLK_A[1]
            setup_port(
                &mut iox,
                IoxPort::PC,
                11,
                Some(IoxFunction::AF1),
                Some(IoxDir::Output),
                Some(IoxDriveStrength::Drive2mA),
                Some(IoxEnable::Disable),
                None,
                None,
            );
            // SPIM_SD[0-3]_A[1]
            for i in 0..3 {
                setup_port(
                    &mut iox,
                    IoxPort::PC,
                    i + 7,
                    Some(IoxFunction::AF1),
                    None,
                    Some(IoxDriveStrength::Drive2mA),
                    Some(IoxEnable::Enable),
                    None,
                    None,
                );
            }
            // SPIM_CSN0_A[0]
            setup_port(
                &mut iox,
                IoxPort::PC,
                12,
                Some(IoxFunction::AF1),
                Some(IoxDir::Output),
                Some(IoxDriveStrength::Drive2mA),
                Some(IoxEnable::Enable),
                None,
                None,
            ); */

            let mut udma_global = GlobalConfig::new(utralib::generated::HW_UDMA_CTRL_BASE as *mut u32);
            udma_global.clock_on(PeriphId::Spim0); // JQSPI1
            // udma_global.clock_on(PeriphId::Spim1); // JPC7_13

            // safety: this is safe because clocks have been set up
            let mut flash_spim = unsafe {
                Spim::new_with_ifram(
                    SpimChannel::Channel0,
                    100_000_000,
                    100_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs0,
                    0,
                    0,
                    None,
                    0, // we will never write to flash
                    4096,
                    Some(8),
                    IframRange::from_raw_parts(SPIM_FLASH_IFRAM_ADDR, SPIM_FLASH_IFRAM_ADDR, 4096 * 2),
                )
            };

            let mut ram_spim = unsafe {
                Spim::new_with_ifram(
                    SpimChannel::Channel0,
                    100_000_000,
                    100_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs1,
                    0,
                    0,
                    None,
                    1024, // this is limited by the page length
                    1024,
                    Some(6),
                    IframRange::from_raw_parts(SPIM_RAM_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR, 4096 * 2),
                )
            };
            // sanity check: read ID
            println!("flash ID: {:x}", flash_spim.mem_read_id());
            println!("ram ID: {:x}", ram_spim.mem_read_id());

            // setup FLASH
            //  - QE enable
            //  - dummy cycles = 8
            flash_spim.mem_write_status_register(0b01_0000_00, 0b10_00_0_111);

            // set SPI devices to QPI mode
            // We expect a MX25L12833F (3.3V) on CS0
            // We expect a ISS66WVS4M8BLL (3.3V) on CS1
            // Both support QPI.
            flash_spim.mem_qpi_mode(true);
            ram_spim.mem_qpi_mode(true);

            // allocate the buf
            let mut buf = RawPage { data: [0u8; 4096] };
            // fetch the header
            flash_spim.mem_read(SWAP_IMG_START as u32, &mut buf.data);

            // safety: buf.data is aligned to 4096-byte boundary and filled with initialized data
            let ssh: &SwapSourceHeader = unsafe { &*(buf.data.as_ptr() as *const &SwapSourceHeader) };
            let mut hal = SwapHal {
                image_start: SWAP_IMG_START as usize + 4096,
                image_mac_start: SWAP_IMG_START as usize + 4096 + ssh.mac_offset as usize,
                partial_nonce: [0u8; 8],
                aad_storage: [0u8; 64],
                aad_len: 0,
                cipher: Aes256GcmSiv::new((&swap.key).into()),
                flash_spim,
                ram_spim,
                iox,
                udma_global,
                buf,
            };
            hal.aad_storage[..ssh.aad_len as usize].copy_from_slice(&ssh.aad[..ssh.aad_len as usize]);
            hal.aad_len = ssh.aad_len as usize;
            hal.partial_nonce.copy_from_slice(&ssh.parital_nonce);
            Some(hal)
        } else {
            None
        }
    }

    fn aad(&self) -> &[u8] { &self.aad_storage[..self.aad_len] }

    pub fn decrypt_page_at(&mut self, offset: usize) -> &[u8] {
        assert!((offset & 0xFFF) == 0, "offset is not page-aligned");
        self.flash_spim.mem_read((self.image_start + offset) as u32, &mut self.buf.data);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[..4].copy_from_slice(&(offset as u32).to_be_bytes());
        nonce[4..].copy_from_slice(&self.partial_nonce);
        let mut tag = [0u8; size_of::<Tag>()];
        // avoid mutable borrow problem by copying AAD to a dedicated location:
        // we could do this with a Refcell but I suspect this is probably actually
        // cheaper than the Refcell bookkeeping.
        let mut aad = [0u8; 64];
        aad[..self.aad_len].copy_from_slice(self.aad());
        self.flash_spim
            .mem_read((self.image_mac_start + (offset / 4096) * size_of::<Tag>()) as u32, &mut tag);
        match self.cipher.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            &aad[..self.aad_len],
            &mut self.buf.data,
            (&tag).into(),
        ) {
            Ok(_) => &self.buf.data,
            Err(e) => panic!("Decryption error in swap: {:?}", e),
        }
    }
}
