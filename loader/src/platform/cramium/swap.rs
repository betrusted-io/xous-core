use core::mem::size_of;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, KeyInit, Nonce, Tag};
use cramium_hal::ifram::IframRange;
use cramium_hal::iox::*;
use cramium_hal::sce;
use cramium_hal::udma::*;
use loader::swap::SPIM_RAM_IFRAM_ADDR;
use loader::APP_UART_IFRAM_ADDR;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::bootconfig::BootConfig;
use crate::swap::*;
use crate::*;

/// hard coded at offset 0 of SPI FLASH for now, until we figure out if and how to move this around.
const SWAP_IMG_START: usize = 0;

pub struct SwapHal {
    image_start: usize,
    image_mac_start: usize,
    partial_nonce: [u8; 8],
    // overflow AAD with panic if it's longer than this!
    aad_storage: [u8; 64],
    aad_len: usize,
    swap_mac_start: usize,
    swap_mac_len: usize,
    buf_addr: usize,
    ram_swap_key: [u8; 32],
    src_cipher: Aes256GcmSiv,
    dst_cipher: Aes256GcmSiv,
    flash_spim: Spim,
    ram_spim: Spim,
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
            let mut udma_global = GlobalConfig::new(utralib::generated::HW_UDMA_CTRL_BASE as *mut u32);

            // setup the I/O pins
            let mut iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
            #[cfg(feature = "spi-alt-channel")]
            let channel = {
                // JQSPI1
                // SPIM_CLK_A[0]
                setup_port(
                    &mut iox,
                    IoxPort::PD,
                    4,
                    Some(IoxFunction::AF1),
                    Some(IoxDir::Output),
                    Some(IoxDriveStrength::Drive4mA),
                    Some(IoxEnable::Enable),
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
                udma_global.clock_on(PeriphId::Spim0); // JQSPI1
                SpimChannel::Channel0
            };
            #[cfg(not(feature = "spi-alt-channel"))]
            let channel = {
                // JPC7_13
                // SPIM_CLK_A[1]
                setup_port(
                    &mut iox,
                    IoxPort::PC,
                    11,
                    Some(IoxFunction::AF1),
                    Some(IoxDir::Output),
                    Some(IoxDriveStrength::Drive4mA),
                    Some(IoxEnable::Enable),
                    None,
                    None,
                );
                // SPIM_SD[0-3]_A[1]
                for i in 7..11 {
                    setup_port(
                        &mut iox,
                        IoxPort::PC,
                        i,
                        Some(IoxFunction::AF1),
                        None,
                        Some(IoxDriveStrength::Drive2mA),
                        Some(IoxEnable::Enable),
                        None,
                        None,
                    );
                }
                // SPIM_CSN0_A[1]
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
                );
                // SPIM_CSN0_A[1]
                setup_port(
                    &mut iox,
                    IoxPort::PC,
                    13,
                    Some(IoxFunction::AF1),
                    Some(IoxDir::Output),
                    Some(IoxDriveStrength::Drive2mA),
                    Some(IoxEnable::Enable),
                    None,
                    None,
                );
                udma_global.clock_on(PeriphId::Spim1); // JPC7_13
                SpimChannel::Channel1
            };

            // safety: this is safe because clocks have been set up
            let mut flash_spim = unsafe {
                Spim::new_with_ifram(
                    channel,
                    25_000_000,
                    50_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs0,
                    0,
                    0,
                    None,
                    16, // just enough space to send commands
                    4096,
                    Some(6),
                    None,
                    IframRange::from_raw_parts(SPIM_FLASH_IFRAM_ADDR, SPIM_FLASH_IFRAM_ADDR, 4096 * 2),
                )
            };

            let mut ram_spim = unsafe {
                Spim::new_with_ifram(
                    channel,
                    25_000_000,
                    50_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs1,
                    0,
                    0,
                    None,
                    1024, // this is limited by the page length
                    1024,
                    Some(6),
                    None,
                    IframRange::from_raw_parts(SPIM_RAM_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR, 4096),
                )
            };
            // turn off QPI mode, in case it was set from a reboot in a bad state
            flash_spim.mem_qpi_mode(false);
            ram_spim.mem_qpi_mode(false);

            // sanity check: read ID
            let flash_id = flash_spim.mem_read_id_flash();
            let ram_id = ram_spim.mem_read_id_ram();
            crate::println!("flash ID: {:x}", flash_id);
            crate::println!("ram ID: {:x}", ram_id);
            // density 18, memory type 20, mfg ID C2 ==> MX25L128833F
            assert!(flash_id & 0xFF_FF_FF == 0x1820C2);
            // KGD 5D, mfg ID 9D; remainder of bits are part of the EID
            assert!(ram_id & 0xFF_FF == 0x5D9D);

            // setup FLASH
            //  - QE enable
            //  - dummy cycles = 6
            // This is not necessary: QE should be set by the external FLASH
            // provisioning routine.
            // TODO: figure out why writing this causes reads to break (every other read works)
            // flash_spim.mem_write_status_register(0b01_0000_00, 0b00_00_0_111);

            // set SPI devices to QPI mode
            // We expect a MX25L12833F (3.3V) on CS0
            // We expect a ISS66WVS4M8BLL (3.3V) on CS1
            // Both support QPI.
            flash_spim.mem_qpi_mode(true);
            ram_spim.mem_qpi_mode(true);

            // re-check the ID to confirm we entered QPI mode correctly
            let flash_id = flash_spim.mem_read_id_flash();
            let ram_id = ram_spim.mem_read_id_ram();
            crate::println!("QPI flash ID: {:x}", flash_id);
            crate::println!("QPI ram ID: {:x}", ram_id);
            // density 18, memory type 20, mfg ID C2 ==> MX25L128833F
            assert!(flash_id & 0xFF_FF_FF == 0x1820C2);
            // KGD 5D, mfg ID 9D; remainder of bits are part of the EID
            assert!(ram_id & 0xFF_FF == 0x5D9D);

            // allocate the buf
            let mut buf = RawPage { data: [0u8; 4096] };
            // fetch the header
            if !flash_spim.mem_read(SWAP_IMG_START as u32, &mut buf.data, false) {
                println!("Timeout reading FLASH_SPIM");
            };

            // compute offsets for swap
            let ram_size_actual = crate::swap::derive_usable_swap(swap.ram_size as usize);
            let mac_size = crate::swap::derive_mac_size(swap.ram_size as usize);
            if SDBG {
                println!(
                    "mac area size: {:x}, ram_size_actual: {:x}, swap.ram_size: {:x}, mac offset: {:x}",
                    mac_size,
                    ram_size_actual,
                    swap.ram_size,
                    swap.ram_offset as usize + ram_size_actual
                );
            }

            // generate a random key for swap
            let mut trng = sce::trng::Trng::new(utralib::generated::HW_TRNG_BASE as usize);
            trng.setup_raw_generation(32);
            let mut seed = 0u64;
            seed |= trng.get_u32().expect("TRNG error") as u64;
            seed |= (trng.get_u32().expect("TRNG error") as u64) << 32;
            let mut cstrng = ChaCha8Rng::seed_from_u64(seed);
            // accumulate more TRNG data, because I don't trust it.
            // 1. whiten the existing TRNG data with ChaCha8
            // 2. XOR in another 32 bits of TRNG data
            // 3. create a new ChaCha8 from the resulting data
            for _ in 0..16 {
                seed = cstrng.next_u64();
                seed ^= trng.get_u32().expect("TRNG error") as u64;
                cstrng = ChaCha8Rng::seed_from_u64(seed);
            }
            // now we might have a properly seeded cryptographically secure TRNG...
            let mut dest_key = [0u8; 32];
            for word in dest_key.chunks_mut(core::mem::size_of::<u32>()) {
                word.copy_from_slice(&cstrng.next_u32().to_be_bytes());
            }

            // safety: buf.data is aligned to 4096-byte boundary and filled with initialized data
            let ssh: &SwapSourceHeader =
                unsafe { (buf.data.as_ptr() as *const SwapSourceHeader).as_ref().unwrap() };
            #[cfg(feature = "debug-print")]
            {
                println!("SwapSourceHeader: {:x?}", ssh);
                println!("Swap key: {:x?}", &swap.key);
                println!("Dest key: {:x?}", &dest_key);
            }
            let mut hal = SwapHal {
                image_start: SWAP_IMG_START as usize + 4096,
                image_mac_start: SWAP_IMG_START as usize + 4096 + ssh.mac_offset as usize,
                partial_nonce: [0u8; 8],
                aad_storage: [0u8; 64],
                aad_len: 0,
                src_cipher: Aes256GcmSiv::new((&swap.key).into()),
                flash_spim,
                ram_spim,
                swap_mac_start: ram_size_actual,
                swap_mac_len: mac_size,
                dst_cipher: Aes256GcmSiv::new((&dest_key).into()),
                buf_addr: 0,
                buf,
                ram_swap_key: dest_key,
            };
            hal.aad_storage[..ssh.aad_len as usize].copy_from_slice(&ssh.aad[..ssh.aad_len as usize]);
            hal.aad_len = ssh.aad_len as usize;
            hal.partial_nonce.copy_from_slice(&ssh.partial_nonce);
            Some(hal)
        } else {
            None
        }
    }

    pub fn get_swap_key(&self) -> &[u8] { &self.ram_swap_key }

    fn aad(&self) -> &[u8] { &self.aad_storage[..self.aad_len] }

    /// `offset` is the offset from the beginning of the encrypted region (not full disk region)
    pub fn decrypt_src_page_at(&mut self, offset: usize) -> &[u8] {
        assert!((offset & 0xFFF) == 0, "offset is not page-aligned");
        self.buf_addr = offset;
        self.flash_spim.mem_read((self.image_start + offset) as u32, &mut self.buf.data, false);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[..4].copy_from_slice(&(offset as u32).to_be_bytes());
        nonce[4..].copy_from_slice(&self.partial_nonce);
        let mut tag = [0u8; size_of::<Tag>()];
        // avoid mutable borrow problem by copying AAD to a dedicated location:
        // we could do this with a Refcell but I suspect this is probably actually
        // cheaper than the Refcell bookkeeping.
        let mut aad = [0u8; 64];
        aad[..self.aad_len].copy_from_slice(self.aad());
        self.flash_spim.mem_read(
            (self.image_mac_start + (offset / 4096) * size_of::<Tag>()) as u32,
            &mut tag,
            false,
        );
        match self.src_cipher.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            &aad[..self.aad_len],
            &mut self.buf.data,
            (&tag).into(),
        ) {
            Ok(_) => &self.buf.data,
            Err(e) => panic!("Decryption error from swap image: {:?}", e),
        }
    }

    pub fn decrypt_page_addr(&self) -> usize { self.buf_addr }

    pub fn buf_as_mut(&mut self) -> &mut [u8] { &mut self.buf.data }

    pub fn buf_as_ref(&self) -> &[u8] { &self.buf.data }

    pub fn mac_base_bounds(&self) -> (u32, u32) { (self.swap_mac_start as u32, self.swap_mac_len as u32) }

    /// Swap count is fixed at 0 by this routine. The data to be encrypted is
    /// assumed to already be in `self.buf`
    pub fn encrypt_swap_to(&mut self, buf: &mut [u8], dest_offset: usize, src_vaddr: usize, src_pid: u8) {
        assert!(buf.len() == PAGE_SIZE);
        assert!(dest_offset & (PAGE_SIZE - 1) == 0);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[0..4].copy_from_slice(&[0u8; 4]); // this is the `swap_count` field
        nonce[5] = src_pid;
        let ppage_masked = dest_offset & !(PAGE_SIZE - 1);
        nonce[6..9].copy_from_slice(&(ppage_masked as u32).to_be_bytes()[..3]);
        let vpage_masked = src_vaddr & !(PAGE_SIZE - 1);
        nonce[9..12].copy_from_slice(&(vpage_masked as u32).to_be_bytes()[..3]);
        let aad: &[u8] = &[];
        match self.dst_cipher.encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, buf) {
            Ok(tag) => {
                self.ram_spim.mem_ram_write(dest_offset as u32, buf, false);
                self.ram_spim.mem_ram_write(
                    (self.swap_mac_start + (dest_offset / PAGE_SIZE) * size_of::<Tag>()) as u32,
                    tag.as_slice(),
                    false,
                );
            }
            Err(e) => panic!("Encryption error to swap ram: {:?}", e),
        }
    }

    /// Swap count is fixed at 0 by this routine. The data to be encrypted is
    /// assumed to already be in `self.buf`
    pub fn decrypt_swap_from(&mut self, src_offset: usize, dst_vaddr: usize, dst_pid: u8) -> &[u8] {
        assert!(src_offset & (PAGE_SIZE - 1) == 0);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[0..4].copy_from_slice(&[0u8; 4]); // this is the `swap_count` field
        nonce[5] = dst_pid;
        let ppage_masked = src_offset & !(PAGE_SIZE - 1);
        nonce[6..9].copy_from_slice(&(ppage_masked as u32).to_be_bytes()[..3]);
        let vpage_masked = dst_vaddr & !(PAGE_SIZE - 1);
        nonce[9..12].copy_from_slice(&(vpage_masked as u32).to_be_bytes()[..3]);
        let aad: &[u8] = &[];
        let mut tag = [0u8; size_of::<Tag>()];
        self.ram_spim.mem_read(
            (self.swap_mac_start + (src_offset / PAGE_SIZE) * size_of::<Tag>()) as u32,
            &mut tag,
            false,
        );
        self.ram_spim.mem_read(src_offset as u32, &mut self.buf.data, false);
        match self.dst_cipher.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            aad,
            &mut self.buf.data,
            (&tag).into(),
        ) {
            Ok(_) => &self.buf.data,
            Err(e) => panic!("Decryption error from swap ram: {:?}", e),
        }
    }

    /// Grabs a slice of the internal buffer. Useful for re-using the decrypted page
    /// between elements of the bootloader (saving us from redundant decrypt ops),
    /// but extremely unsafe because we have to track the use of this buffer manually.
    pub unsafe fn get_decrypt(&self) -> &[u8] { &self.buf.data }
}

/// Function for initializing any PTE mappings needed by the swapper to be functional
/// at boot -- the swapper userspace cannot itself invoke page maps to initialize itself
/// because this would cause a circular dependency.
pub fn userspace_maps(cfg: &mut BootConfig) {
    let tt_address = cfg.processes[SWAPPER_PID as usize - 1].satp << 12;
    let root = unsafe { &mut *(tt_address as *mut crate::PageTable) };

    // map the IFRAM structure into userspace
    // use map_page_32 because we don't track this in the RPT.
    cfg.map_page_32(
        root,
        SPIM_RAM_IFRAM_ADDR,
        SWAP_HAL_VADDR,
        FLG_R | FLG_W | FLG_U | FLG_VALID,
        SWAPPER_PID,
    );

    let mut iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
    iox.set_alternate_function(IoxPort::PD, 2, IoxFunction::AF2);
    iox.set_alternate_function(IoxPort::PD, 3, IoxFunction::AF2);
    // rx as input, with pull-up
    iox.set_gpio_dir(IoxPort::PD, 2, IoxDir::Input);
    iox.set_gpio_pullup(IoxPort::PD, 2, IoxEnable::Enable);
    // tx as output
    iox.set_gpio_dir(IoxPort::PD, 3, IoxDir::Output);

    // Set up the UDMA_UART block to the correct baud rate and enable status
    let mut udma_global =
        cramium_hal::udma::GlobalConfig::new(utralib::utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udma_global.clock_on(cramium_hal::udma::PeriphId::Uart0);
    udma_global.map_event(
        cramium_hal::udma::PeriphId::Uart0,
        cramium_hal::udma::PeriphEventType::Uart(cramium_hal::udma::EventUartOffset::Rx),
        cramium_hal::udma::EventChannel::Channel2,
    );
    udma_global.map_event(
        cramium_hal::udma::PeriphId::Uart0,
        cramium_hal::udma::PeriphEventType::Uart(cramium_hal::udma::EventUartOffset::Tx),
        cramium_hal::udma::EventChannel::Channel3,
    );

    let baudrate: u32 = 115200;
    let perclk = 100_000_000; // TODO: turn this into a symbolic const, or better yet, pass in from the loader
    let freq: u32 = perclk / 2;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = APP_UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        cramium_hal::udma::Uart::get_handle(
            utralib::utra::udma_uart_0::HW_UDMA_UART_0_BASE,
            uart_buf_addr,
            uart_buf_addr,
        )
    };
    crate::println!("Baud freq is {} Hz, baudrate is {}", freq, baudrate);
    udma_uart.set_baud(baudrate, freq);

    udma_uart.write(b"APP UART INIT OK\n\r");

    // map the debug UART HW page
    cfg.map_page_32(
        root,
        // TODO: use PD2/3 AF2 for this UART; set up the IOs for this
        utralib::utra::udma_uart_0::HW_UDMA_UART_0_BASE,
        SWAP_APP_UART_VADDR,
        FLG_R | FLG_W | FLG_U | FLG_VALID,
        SWAPPER_PID,
    );
    // map the debug UART IFRAM page
    cfg.map_page_32(
        root,
        // TODO: use PD2/3 AF2 for this UART; set up the IOs for this
        APP_UART_IFRAM_ADDR,
        SWAP_APP_UART_IFRAM_VADDR,
        FLG_R | FLG_W | FLG_U | FLG_VALID,
        SWAPPER_PID,
    );
}
