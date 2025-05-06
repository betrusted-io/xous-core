use core::convert::TryInto;
use core::fmt::Write;

use cramium_api::*;
use cramium_hal::board::SPIM_FLASH_IFRAM_ADDR;
use cramium_hal::ifram::IframRange;
use cramium_hal::iox::Iox;
use cramium_hal::mbox::{
    MBOX_PROTOCOL_REV, Mbox, MboxError, MboxToCm7Pkt, PAYLOAD_LEN_WORDS, RERAM_PAGE_SIZE_BYTES, ToCm7Op,
    ToRvOp,
};
use cramium_hal::sh1107::{Mono, Oled128x128};
use cramium_hal::udma;
use cramium_hal::udma::*;
use cramium_hal::usb::driver::UsbDeviceState;
use ed25519_dalek::{Digest, Signature, VerifyingKey};
use sha2::Sha512;
use simple_fatfs::PathBuf;
use utralib::generated::*;
use ux_api::minigfx::{FrameBuffer, Point};

use crate::SIGBLOCK_SIZE;
use crate::platform::cramium::gfx;
use crate::platform::cramium::sha512_digest::Sha512Prehash;
use crate::platform::cramium::usb::{self, SliceCursor, disable_all_irqs};
use crate::platform::delay;

// TODO:
//   - Port unicode font drawing into loader
//   - Support localization

const TEXT_MIDLINE: isize = 51;

// Empirically measured PORTSC when the port is unplugged. This might be a brittle way
// to detect if the device is unplugged.
const DISCONNECT_STATE: u32 = 0x40b;

// loader is not updateable here because we're XIP. But we can update these other images:
const SWAP_NAME: &'static str = "SWAP.IMG";
const KERNEL_NAME: &'static str = "XOUS.BIN";
#[allow(dead_code)]
const SWAP_BASE_ADDR: usize = 0;
const KERNEL_BASE_ADDR: usize = crate::platform::KERNEL_OFFSET;
const DEV_PUBKEY: [u8; 32] = [
    0x1c, 0x9b, 0xea, 0xe3, 0x2a, 0xea, 0xc8, 0x75, 0x07, 0xc1, 0x80, 0x94, 0x38, 0x7e, 0xff, 0x1c, 0x74,
    0x61, 0x42, 0x82, 0xaf, 0xfd, 0x81, 0x52, 0xd8, 0x71, 0x35, 0x2e, 0xdf, 0x3f, 0x58, 0xbb,
];
#[repr(C)]
struct SignatureInFlash {
    pub version: u32,
    pub signed_len: u32,
    pub signature: [u8; 64],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyPress {
    Up,
    Down,
    Left,
    Right,
    Select,
    Start,
    A,
    B,
    Invalid,
    None,
}

pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [KeyPress; 4] {
    let mut key_presses: [KeyPress; 4] = [KeyPress::None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = match (row, col) {
                        (0, 2) => KeyPress::None, // KeyPress::Select, None due to broken hardware
                        (2, 1) => KeyPress::Start,
                        (1, 2) => KeyPress::None, // KeyPress::Left, None due to broken hardware
                        (2, 2) => KeyPress::None, // None due to broken hardware
                        (1, 1) => KeyPress::Up,
                        (0, 1) => KeyPress::Down,
                        (2, 0) => KeyPress::Right,
                        (0, 0) => KeyPress::A,
                        (1, 0) => KeyPress::B,
                        _ => KeyPress::Invalid,
                    };
                    key_press_index += 1;
                }
            }
        }
        iox.set_gpio_pin_value(*port, *pin, IoxValue::High);
    }
    key_presses
}

/// Checks to see if the necessary conditions for an update are met
pub fn process_update(perclk: u32) {
    crate::println!("entering process_update");
    // Placeholder:
    // Remember to lock the root keys before processing any updates
    crate::platform::cramium::verifier::lifecycle_lock_root();

    crate::println!("waiting for button press");
    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let mut udma_global = udma::GlobalConfig::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);

    let iox_kbd = iox.clone();
    let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(
        cramium_hal::sh1107::MainThreadToken::new(),
        perclk,
        &mut iox,
        &mut udma_global,
    );

    crate::platform::cramium::bootlogo::show_logo(&mut sh1107);
    gfx::msg(
        &mut sh1107,
        "    START to boot",
        Point::new(0, 115 - 16),
        Mono::White.into(),
        Mono::Black.into(),
    );
    gfx::msg(&mut sh1107, "   DOWN to update", Point::new(0, 115), Mono::White.into(), Mono::Black.into());

    sh1107.draw();

    // setup IO pins to check for update viability
    let (rows, cols) = cramium_hal::board::baosec::setup_kb_pins(&iox_kbd);

    let mut key_pressed = false;
    let mut do_update = false;
    while !key_pressed {
        let kps = scan_keyboard(&iox_kbd, &rows, &cols);
        for kp in kps {
            if kp != KeyPress::None {
                crate::println!("Got key: {:?}", kp);
                key_pressed = true;
            }
            if kp == KeyPress::Down {
                do_update = true;
            }
        }
    }

    sh1107.clear();

    if do_update {
        gfx::msg(
            &mut sh1107,
            "Connect to USB",
            Point::new(16, TEXT_MIDLINE),
            Mono::White.into(),
            Mono::Black.into(),
        );
        sh1107.draw();

        // safety: this is safe because we're calling this before any access to `USB` static mut
        // state, and we also understand that the .data section doesn't exist in the loader and
        // we've taken countermeasures to initialize everything "from code", i.e. not relying
        // on static compile-time assignments for the static mut state.
        unsafe { crate::platform::cramium::usb::init_usb() };

        // Below is all unsafe because USB is global mutable state
        unsafe {
            if let Some(ref mut usb_ref) = crate::platform::cramium::usb::USB {
                let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
                usb.reset();
                let mut poweron = 0;
                loop {
                    usb.udc_handle_interrupt();
                    if usb.pp() {
                        poweron += 1; // .pp() is a sham. MPW has no way to tell if power is applied. This needs to be fixed for NTO.
                    }
                    crate::platform::delay(100);
                    if poweron >= 4 {
                        break;
                    }
                }
                usb.reset();
                usb.init();
                usb.start();
                usb.update_current_speed();
                // IRQ enable must happen without dependency on the hardware lock
                usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
                usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);

                let mut last_usb_state = usb.get_device_state();
                let mut portsc = usb.portsc_val();
                crate::println!("USB state: {:?}, {:x}", last_usb_state, portsc);
                loop {
                    let kps = scan_keyboard(&iox_kbd, &rows, &cols);
                    // only consider the first key returned in case of multi-key hit, for simplicity
                    if kps[0] == KeyPress::Down {
                        break;
                    } else if kps[0] != KeyPress::None {
                        crate::println!("Got key {:?}; ignoring", kps[0]);
                    }
                    let new_usb_state = usb.get_device_state();
                    let new_portsc = usb.portsc_val();
                    // alternately, break out of the loop when USB is disconnected
                    if new_portsc != portsc {
                        crate::println!("PP: {:x}", portsc);
                        portsc = new_portsc;
                        if portsc == DISCONNECT_STATE && new_usb_state == UsbDeviceState::Configured {
                            break;
                        }
                    }
                    if new_usb_state != last_usb_state {
                        crate::println!("USB state: {:?}", new_usb_state);
                        if new_usb_state == UsbDeviceState::Configured {
                            sh1107.clear();
                            gfx::msg(
                                &mut sh1107,
                                "Copy files to device",
                                Point::new(6, TEXT_MIDLINE),
                                Mono::White.into(),
                                Mono::Black.into(),
                            );
                            gfx::msg(
                                &mut sh1107,
                                "Press DOWN",
                                Point::new(22, TEXT_MIDLINE + 18),
                                Mono::Black.into(),
                                Mono::White.into(),
                            );
                            gfx::msg(
                                &mut sh1107,
                                "when finished!",
                                Point::new(19, TEXT_MIDLINE + 32),
                                Mono::Black.into(),
                                Mono::White.into(),
                            );
                            sh1107.draw();
                            last_usb_state = new_usb_state;
                        }
                    }
                }

                let disk = usb::conjure_disk();
                let mut cursor = SliceCursor::new(disk);

                // We can either pass by value of by (mutable) reference
                let mut fs = simple_fatfs::FileSystem::from_storage(&mut cursor).unwrap();
                match fs.read_dir(PathBuf::from("/")) {
                    Ok(dir) => {
                        for entry in dir {
                            crate::println!("{:?}", entry);
                            if let Some(file_name) = entry.path().file_name() {
                                if file_name.to_ascii_uppercase() == KERNEL_NAME {
                                    let f = fs.get_file(entry.path().clone()).expect("file error");
                                    let sector_offset = f.sector_offset();
                                    crate::println!("sector offset: {}", sector_offset);
                                    let disk_access = usb::conjure_disk();
                                    crate::println!(
                                        "{:x?}",
                                        &disk_access
                                            [sector_offset as usize * 512..sector_offset as usize * 512 + 32]
                                    );
                                    let pubkey = VerifyingKey::from_bytes(&DEV_PUBKEY)
                                        .expect("public key was not valid");
                                    crate::println!("pubkey as reconstituted: {:x?}", pubkey);

                                    let k_start = sector_offset as usize * 512;
                                    let sig_region = &disk_access
                                        [k_start..k_start + core::mem::size_of::<SignatureInFlash>()];
                                    let sig_rec: &SignatureInFlash =
                                        (sig_region.as_ptr() as *const SignatureInFlash).as_ref().unwrap(); // this pointer better not be null, we just created it!
                                    let sig = Signature::from_bytes(&sig_rec.signature);

                                    let kern_len = sig_rec.signed_len as usize;
                                    crate::println!("recorded kernel len: {} bytes", kern_len);
                                    crate::println!("verifying with signature {:x?}", sig_rec.signature);
                                    crate::println!("verifying with pubkey {:x?}", pubkey.to_bytes());

                                    let mut h: Sha512 = Sha512::new();
                                    let image = &disk_access[k_start + SIGBLOCK_SIZE
                                        ..k_start + SIGBLOCK_SIZE + sig_rec.signed_len as usize];
                                    crate::println!("image bytes: {:x?}", &image[..16]);
                                    h.update(&image);
                                    let hash = h.finalize();
                                    let mut ph = Sha512Prehash::new();
                                    ph.set_prehash(hash.as_slice().try_into().unwrap());
                                    let v_result = pubkey.verify_prehashed(ph, None, &sig);
                                    if let Err(e) = v_result {
                                        crate::println!("error verifying signature: {:?}", e);
                                        gfx::msg(
                                            &mut sh1107,
                                            "Kernel invalid!",
                                            Point::new(10, TEXT_MIDLINE),
                                            Mono::White.into(),
                                            Mono::Black.into(),
                                        );
                                        sh1107.draw();
                                        sh1107.clear();
                                        return;
                                    }
                                    crate::println!("Kernel image is good!");
                                    let mut mbox = cramium_hal::mbox::Mbox::new();
                                    // concatenate this with e.g. .min(0x2000) to shorten the run, if needed
                                    let test_len = sig_rec.signed_len as usize;
                                    let check_slice = core::slice::from_raw_parts(
                                        (KERNEL_BASE_ADDR + utralib::HW_RERAM_MEM) as *const u8,
                                        0x3000,
                                    );
                                    crate::println!(
                                        "data before: {:x?} .. {:x?}",
                                        &check_slice[..32],
                                        &check_slice[0x1000..0x1000 + 32]
                                    );

                                    progress_bar(&mut sh1107, 0);

                                    // nearest event multiple of RERAM_PAGE_SIZE_BYTES that fits into
                                    // our available payload length
                                    const BLOCKLEN_BYTES: usize = (PAYLOAD_LEN_WORDS * size_of::<u32>()
                                        / RERAM_PAGE_SIZE_BYTES)
                                        * RERAM_PAGE_SIZE_BYTES;
                                    let total_len = SIGBLOCK_SIZE + test_len;
                                    for (i, byte_chunk) in disk_access
                                        [k_start..k_start + SIGBLOCK_SIZE + test_len as usize]
                                        .chunks(BLOCKLEN_BYTES)
                                        .enumerate()
                                    {
                                        // +2 words are for the address / length fields required by the
                                        // protocol
                                        let mut buffer = [0u32; BLOCKLEN_BYTES / size_of::<u32>() + 2];
                                        for (src, dst) in byte_chunk.chunks(4).zip(buffer[2..].iter_mut()) {
                                            *dst = u32::from_le_bytes(src.try_into().unwrap());
                                        }
                                        // now fill in the metadata for the protocol
                                        buffer[0] =
                                            (KERNEL_BASE_ADDR + i * BLOCKLEN_BYTES + utralib::HW_RERAM_MEM)
                                                as u32;
                                        buffer[1] = BLOCKLEN_BYTES as u32;
                                        if i % 8 == 0 {
                                            crate::println!("{:x}: {:x?}", buffer[0], &buffer[2..6]);
                                            progress_bar(&mut sh1107, i * BLOCKLEN_BYTES * 100 / total_len);
                                        }
                                        match write_rram(&mut mbox, &buffer) {
                                            Ok(len) => {
                                                if len != BLOCKLEN_BYTES {
                                                    crate::println!(
                                                        "write length mismatch, got {} expected {} words",
                                                        len,
                                                        BLOCKLEN_BYTES
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                crate::println!("Write failed with {:?}", e);
                                                break;
                                            }
                                        };
                                    }
                                    progress_bar(&mut sh1107, 100);
                                    cache_flush();
                                    crate::println!(
                                        "data after: {:x?} .. {:x?}",
                                        &check_slice[..32],
                                        &check_slice[0x1000..0x1000 + 32]
                                    );
                                } else if file_name.to_ascii_uppercase() == SWAP_NAME {
                                    // This has a totally different method, as it's writing to SPI FLASH
                                    crate::println!("Found swap image");
                                    let f = fs.get_file(entry.path().clone()).expect("file error");
                                    let flen = f.file_size();
                                    let swap_offset = f.sector_offset() as usize * 512;
                                    crate::println!("swap offset: {:x}", swap_offset);
                                    let swap_image =
                                        &usb::conjure_disk()[swap_offset..swap_offset + flen as usize];

                                    let ssh: &crate::swap::SwapSourceHeader = (swap_image.as_ptr()
                                        as *const crate::swap::SwapSourceHeader)
                                        .as_ref()
                                        .unwrap();
                                    // minimal image validation - just check that the version number and magic
                                    // number is correct.
                                    if ssh.version == 0x01_01_0000
                                        && u32::from_be_bytes(swap_image[0x14..0x18].try_into().unwrap())
                                            == 0x73776170
                                    {
                                        crate::println!(
                                            "Burning swap image starting at 0x{:x} of len {} bytes",
                                            swap_offset,
                                            flen
                                        );
                                        progress_bar(&mut sh1107, 0);

                                        let udma_global = GlobalConfig::new(
                                            utralib::generated::HW_UDMA_CTRL_BASE as *mut u32,
                                        );

                                        // setup the I/O pins
                                        let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
                                        let channel = cramium_hal::board::setup_memory_pins(&iox);
                                        udma_global.clock_on(PeriphId::from(channel));
                                        // safety: this is safe because clocks have been set up
                                        let mut flash_spim = Spim::new_with_ifram(
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
                                            IframRange::from_raw_parts(
                                                SPIM_FLASH_IFRAM_ADDR,
                                                SPIM_FLASH_IFRAM_ADDR,
                                                4096 * 2,
                                            ),
                                        );

                                        flash_spim.mem_qpi_mode(true);

                                        // NOTE: this programming routine only works for write destinations
                                        // that start aligned to an erase page. In this case, `page_no``
                                        // starts at 0 and increments monatomically, so, this condition is
                                        // satisfied.
                                        for (page_no, chunk) in swap_image.chunks(FLASH_PAGE_LEN).enumerate()
                                        {
                                            // copy chunk to a full-page sized buffer: the last chunk may not
                                            // be exactly a page in length, so this pads it out!
                                            let mut buf = [0u8; 256];
                                            buf[..chunk.len()].copy_from_slice(chunk);

                                            // Erase the page if we're touching it the first time
                                            if ((page_no * FLASH_PAGE_LEN) % 0x1000) == 0 {
                                                flash_spim
                                                    .flash_erase_sector((page_no * FLASH_PAGE_LEN) as u32);
                                            }

                                            flash_spim.mem_flash_write_page(
                                                (page_no * FLASH_PAGE_LEN) as u32,
                                                &buf,
                                            );

                                            if page_no % 32 == 0 {
                                                progress_bar(
                                                    &mut sh1107,
                                                    page_no * FLASH_PAGE_LEN * 100 / flen as usize,
                                                );
                                                crate::println!(
                                                    "{:x}: write {:x?}",
                                                    page_no * FLASH_PAGE_LEN,
                                                    &buf[..16]
                                                );
                                                let mut rbk = [0u8; 16];
                                                flash_spim.mem_read(
                                                    (page_no * FLASH_PAGE_LEN) as u32,
                                                    &mut rbk,
                                                    false,
                                                );
                                                crate::println!("rbk: {:x?}", &rbk);
                                            }
                                        }
                                        progress_bar(&mut sh1107, 100);
                                    } else {
                                        crate::println!(
                                            "Invalid swap image: ver {:x} magic: {:x}",
                                            ssh.version,
                                            u32::from_be_bytes(swap_image[0x14..0x18].try_into().unwrap())
                                        );
                                        gfx::msg(
                                            &mut sh1107,
                                            "Swap invalid!",
                                            Point::new(6, TEXT_MIDLINE),
                                            Mono::White.into(),
                                            Mono::Black.into(),
                                        );
                                        sh1107.draw();
                                        sh1107.clear();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::println!("Couldn't list dir: {:?}", e);
                    }
                }
                // unmap IRQs
                disable_all_irqs();
                usb.stop();
            } else {
                crate::println!("USB core not allocated, can't do update!");
            }
        }
    }

    gfx::msg(
        &mut sh1107,
        "   Booting Xous...",
        Point::new(0, TEXT_MIDLINE),
        Mono::White.into(),
        Mono::Black.into(),
    );
    sh1107.draw();
    sh1107.clear();
}

pub struct UsizeToString {
    buf: [u8; 16], // Enough space for a u32 decimal string
    pos: usize,
}

impl UsizeToString {
    pub const fn new() -> Self { Self { buf: [0; 16], pos: 0 } }

    pub fn as_str(&self) -> &str { core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("") }
}

impl Write for UsizeToString {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
        Ok(())
    }
}

fn progress_bar(sh1107: &mut Oled128x128<'_>, percentage: usize) {
    gfx::msg(sh1107, "Writing, do not", Point::new(8, TEXT_MIDLINE), Mono::White.into(), Mono::Black.into());
    gfx::msg(
        sh1107,
        "reset or turn off!",
        Point::new(4, TEXT_MIDLINE + 14),
        Mono::White.into(),
        Mono::Black.into(),
    );

    let mut usizestr = UsizeToString::new();
    write!(usizestr, "{}%", percentage).ok();
    gfx::msg(
        sh1107,
        usizestr.as_str(),
        Point::new(55, TEXT_MIDLINE + 38),
        Mono::Black.into(),
        Mono::White.into(),
    );
    sh1107.draw();
    sh1107.clear();
}

pub fn write_rram(mbox: &mut Mbox, data: &[u32]) -> Result<usize, MboxError> {
    let write_pkt = MboxToCm7Pkt { version: MBOX_PROTOCOL_REV, opcode: ToCm7Op::FlashWrite, data };
    match mbox.try_send(write_pkt) {
        Ok(_) => {
            crate::platform::delay(10);
            // crate::println!("flash write sent OK");
            let mut timeout = 0;
            while mbox.poll_not_ready() {
                timeout += 1;
                if timeout > 1000 {
                    crate::println!("flash write timeout");
                    return Err(MboxError::NotReady);
                }
                crate::platform::delay(2);
            }
            // now receive the packet
            let mut rx_data = [0u32; 16];
            timeout = 0;
            while !mbox.poll_rx_available() {
                timeout += 1;
                if timeout > 1000 {
                    crate::println!("flash handshake timeout");
                    return Err(MboxError::NotReady);
                }
                crate::platform::delay(2);
            }
            match mbox.try_rx(&mut rx_data) {
                Ok(rx_pkt) => {
                    crate::platform::delay(10);
                    if rx_pkt.version != MBOX_PROTOCOL_REV {
                        crate::println!("Version mismatch {} != {}", rx_pkt.version, MBOX_PROTOCOL_REV);
                    }
                    if rx_pkt.opcode != ToRvOp::RetFlashWrite {
                        crate::println!(
                            "Opcode mismatch {} != {}",
                            rx_pkt.opcode as u16,
                            ToRvOp::RetFlashWrite as u16
                        );
                    }
                    if rx_pkt.len != 1 {
                        crate::println!("Expected length mismatch {} != {}", rx_pkt.len, 1);
                    } else {
                        // crate::println!("Wrote {} bytes", rx_data[0]);
                    }
                    Ok(rx_data[0] as usize)
                }
                Err(e) => {
                    crate::platform::delay(10);
                    crate::println!("Error while deserializing: {:?}\n", e);
                    Err(e)
                }
            }
        }
        Err(e) => {
            delay(10);
            crate::println!("Flash write send error: {:?}", e);
            Err(e)
        }
    }
}

#[inline(always)]
fn cache_flush() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
        ".word 0x500F",
        "nop",
        "nop",
        "nop",
        "nop",
        "nop",
    );
    }
}
