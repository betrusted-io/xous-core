use core::convert::TryInto;
use core::sync::atomic::Ordering;

use bao1x_api::KERNEL_START;
use bao1x_api::*;
use bao1x_hal::iox::Iox;
use bao1x_hal::sh1107::Oled128x128;
use bao1x_hal::udma::*;
use bao1x_hal::usb::driver::CorigineUsb;
use bao1x_hal::usb::driver::*;
use utralib::*;

use super::*;
use crate::{APP_BYTES, BAREMETAL_BYTES, IS_BAOSEC, KERNEL_BYTES, SWAP_BYTES};

pub static TX_IDLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

const UX_UPDATE_INTERVAL_BYTES: u32 = 0x1_0000;

// Locate the "disk"
pub(crate) const RAMDISK_ADDRESS: usize = crate::FREE_MEM_START;
pub(crate) const RAMDISK_ACTUAL_LEN: usize = crate::FREE_MEM_LEN;
pub(crate) const RAMDISK_LEN: usize = 128 * 1024 * 1024; // present as a 128MiB disk. Big enough for "any" update?
pub(crate) const SECTOR_SIZE: u16 = 512;

fn fill_sparse_data(dest: &mut [u8], offset: usize) {
    let dest_start = offset;
    let dest_end = offset + dest.len();
    // initialize to 0
    dest.fill(0);
    // then fill in sparse data
    for &(addr, block) in super::fat32_base::SPARSE_FILE_DATA.iter() {
        // blocks always aligned to the dest, so copies always start from 0 on the block
        assert!(offset % block.len() == 0);
        let block_start = addr;
        let block_end = addr + block.len();

        // Check if block overlaps the destination window
        if block_end <= dest_start || block_start >= dest_end {
            continue; // no overlap
        }

        let write_start_in_dest = block_start.saturating_sub(dest_start);
        let copy_len = (dest.len().saturating_sub(write_start_in_dest)).min(block.len());

        dest[write_start_in_dest..write_start_in_dest + copy_len].copy_from_slice(&block[..copy_len]);
        #[cfg(feature = "alt-boot1")]
        // patch the volume name so we can tell alt-boot apart
        if addr == 0x410000 {
            // patch ALT over BAO. Assumes that the block is aligned.
            dest[write_start_in_dest..write_start_in_dest + 3].copy_from_slice(&[0x41, 0x4c, 0x54])
        }
    }
}

pub(crate) const CSW_ADDR: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + CRG_UDC_APP_BUF_LEN;
pub(crate) const CBW_ADDR: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET;
pub(crate) const EP1_IN_BUF: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + 1024;
pub(crate) const EP1_IN_BUF_LEN: usize = 1024;
#[allow(dead_code)]
pub(crate) const EP1_OUT_BUF: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + 2048;
#[allow(dead_code)]
pub(crate) const EP1_OUT_BUF_LEN: usize = 1024;

pub(crate) const MASS_STORAGE_EPADDR_IN: u8 = 0x81;
pub(crate) const MASS_STORAGE_EPADDR_OUT: u8 = 0x01;

pub const FS_MAX_PKT_SIZE: usize = 64;
pub const HS_MAX_PKT_SIZE: usize = 512;

pub(crate) fn enable_mass_storage_eps(this: &mut CorigineUsb, ep_num: u8) {
    this.ep_enable(ep_num, USB_RECV, HS_MAX_PKT_SIZE as _, EpType::BulkOutbound);
    this.ep_enable(ep_num, USB_SEND, HS_MAX_PKT_SIZE as _, EpType::BulkInbound);
}

// Call from USB_REQ_SET_CONFIGURATION after setting device state to Configured
pub(crate) fn enable_composite_eps(this: &mut CorigineUsb) {
    // MSD endpoints
    enable_mass_storage_eps(this, 1);

    // CDC notification IN (interrupt)
    this.ep_enable(2, USB_SEND, HS_INT_MPS as _, EpType::IntrInbound);

    // CDC data bulk OUT and IN
    this.ep_enable(3, USB_RECV, HS_BULK_MPS as _, EpType::BulkOutbound);
    this.ep_enable(3, USB_SEND, HS_BULK_MPS as _, EpType::BulkInbound);
}

pub fn get_descriptor_request(this: &mut CorigineUsb, value: u16, _index: usize, length: usize) {
    let ep0_buf = unsafe {
        core::slice::from_raw_parts_mut(
            this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
            CRG_UDC_EP0_REQBUFSIZE,
        )
    };

    match (value >> 8) as u8 {
        USB_DT_DEVICE => {
            let mut dd = DeviceDescriptor::composite_with_iad();
            dd.b_max_packet_size0 = 64;
            let len = length.min(core::mem::size_of::<DeviceDescriptor>());
            ep0_buf[..len].copy_from_slice(&dd.as_ref()[..len]);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, len, 0);
        }
        USB_DT_DEVICE_QUALIFIER => {
            let mut q = QualifierDescriptor::default_mass_storage();
            // For composite the fields still apply. Keep class 0.
            q.b_num_configurations = 1;
            let len = length.min(core::mem::size_of::<QualifierDescriptor>());
            ep0_buf[..len].copy_from_slice(&q.as_ref()[..len]);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, len, 0);
        }
        USB_DT_CONFIG => {
            let wrote = write_config_hs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_OTHER_SPEED_CONFIG => {
            let wrote = write_config_fs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_STRING => {
            let id = (value & 0xFF) as u8;
            let len = if id == 0 {
                ep0_buf[..4].copy_from_slice(&[4, USB_DT_STRING, 9, 4]);
                4
            } else {
                let s = match id {
                    1 => MANUFACTURER,
                    2 => PRODUCT,
                    _ => SERIAL,
                };
                let slen = 2 + s.len() * 2;
                ep0_buf[0] = slen as u8;
                ep0_buf[1] = USB_DT_STRING;
                for (dst, &src) in ep0_buf[2..].chunks_exact_mut(2).zip(s.as_bytes()) {
                    dst.copy_from_slice(&(src as u16).to_le_bytes());
                }
                slen
            };
            let buffsize = length.min(len);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_BOS => {
            let total_length =
                core::mem::size_of::<BosDescriptor>() + core::mem::size_of::<ExtCapDescriptor>();
            let bos = BosDescriptor::default_mass_storage(total_length as u16, 1);
            let ext = ExtCapDescriptor::default_mass_storage((0xfa << 8) | (0x3 << 3));
            let response: [&[u8]; 2] = [bos.as_ref(), ext.as_ref()];
            let mut idx = 0;
            for part in response {
                ep0_buf[idx..idx + part.len()].copy_from_slice(part);
                idx += part.len();
            }
            let buffsize = total_length.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        _ => {
            this.ep_halt(0, USB_RECV);
        }
    }
}

pub fn usb_ep1_bulk_out_complete(
    this: &mut CorigineUsb,
    buf_addr: usize,
    info: u32,
    _error: u8,
    _residual: u16,
) {
    let length = info & 0xFFFF;
    let buf = unsafe { core::slice::from_raw_parts(buf_addr as *const u8, info as usize & 0xFFFF) };
    let mut cbw = Cbw::default();
    cbw.as_mut().copy_from_slice(&buf[..size_of::<Cbw>()]);
    let mut csw = Csw::derive();

    if UmsState::CommandPhase == this.ms_state && (length == 31) {
        // CBW
        if cbw.signature == BULK_CBW_SIG {
            csw.signature = BULK_CSW_SIG;
            csw.tag = cbw.tag;
            csw.update_hw();
            process_mass_storage_command(this, cbw);
            // invalid_cbw = 0;
        } else {
            crate::println_d!("Invalid CBW, HALT");
            this.ep_halt(1, USB_SEND);
            this.ep_halt(1, USB_RECV);
            // invalid_cbw = 1;
        }
    } else if UmsState::CommandPhase == this.ms_state && (length != 31) {
        crate::println_d!("invalid command");
        this.ep_halt(1, USB_SEND);
        this.ep_halt(1, USB_RECV);
        // invalid_cbw = 1;
    } else if UmsState::DataPhase == this.ms_state {
        // crate::println_d!("data");
        //DATA
        if let Some((write_offset, len)) = this.callback_wr.take() {
            let app_buf = conjure_app_buf();
            let disk = conjure_disk();
            // process the received data as if it were a uf2 sector
            for (i, chunk) in app_buf[..len].chunks(512).enumerate() {
                let (new_block, uf2_data) = critical_section::with(|cs| {
                    super::glue::SECTOR
                        .borrow(cs)
                        .borrow_mut()
                        .extend_from_slice(write_offset + i * 512, chunk)
                });
                const APP_RAM_ADDR: usize =
                    utralib::HW_RERAM_MEM + bao1x_api::offsets::dabao::APP_RRAM_OFFSET;
                const STORAGE_END_ADDR: usize = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
                const SWAP_END_ADDR: usize =
                    bao1x_api::offsets::SWAP_START_UF2 + bao1x_api::offsets::SWAP_UF2_LEN;

                #[cfg(not(feature = "alt-boot1"))]
                const START_RANGE: usize = bao1x_api::BAREMETAL_START;
                #[cfg(feature = "alt-boot1")]
                const START_RANGE: usize = bao1x_api::BOOT1_START;
                // program the flash if a valid u2f block was found
                if let Some(record) = uf2_data {
                    if matches!(record.address() as usize, START_RANGE..=STORAGE_END_ADDR)
                        && record.family() == bao1x_api::BAOCHIP_1X_UF2_FAMILY
                    {
                        let mut rram = bao1x_hal::rram::Reram::new();
                        let offset = record.address() as usize - utralib::HW_RERAM_MEM;
                        match rram.write_slice(offset, record.data()) {
                            Err(e) => crate::print_d!("Write error {:?} @ {:x}", e, offset),
                            Ok(_) => (),
                        };
                        /*
                        crate::println_d!(
                            "Wrote {} to 0x{:x}: {:x?}",
                            record.data().len(),
                            record.address(),
                            &record.data()[..8]
                        );
                        */
                    } else if record.address() as usize >= bao1x_api::SWAP_START_UF2
                        && (record.address() as usize) < bao1x_api::SWAP_START_UF2 + bao1x_api::SWAP_UF2_LEN
                        && record.family() == bao1x_api::BAOCHIP_1X_UF2_FAMILY
                    {
                        let spim_addr = record.address() & (bao1x_api::SWAP_UF2_LEN as u32 - 1);
                        /*
                        crate::println_d!(
                            "Received {} to 0x{:x}: {:x?}",
                            record.data().len(),
                            spim_addr,
                            &record.data()[..8]
                        );
                        */
                        match critical_section::with(|cs| {
                            if let Some(assembler) = &mut *super::glue::SECTOR_TRACKER.borrow(cs).borrow_mut()
                            {
                                assembler.add_page(spim_addr as usize, record.data().try_into().unwrap())
                            } else {
                                Err(
                                    "Write to swap received but no swap is available on this board. Ignoring!",
                                )
                            }
                        }) {
                            Ok(_) => (),
                            Err(s) => crate::println_d!("Swap update error: {}", s),
                        }
                    } else {
                        crate::println_d!("Invalid write address {:x}, block ignored!", record.address());
                    }
                    // do some bookkeeping for the UI
                    let (partition, status) = if !IS_BAOSEC.load(Ordering::SeqCst) {
                        if matches!(record.address() as usize, START_RANGE..=APP_RAM_ADDR) {
                            ("core", BAREMETAL_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst))
                        } else if matches!(record.address() as usize, APP_RAM_ADDR..=STORAGE_END_ADDR) {
                            ("app", APP_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst))
                        } else {
                            ("none", 0)
                        }
                    } else {
                        if matches!(record.address() as usize, START_RANGE..=KERNEL_START) {
                            (
                                "loader",
                                BAREMETAL_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst),
                            )
                        } else if matches!(record.address() as usize, KERNEL_START..=STORAGE_END_ADDR) {
                            ("kernel", KERNEL_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst))
                        } else if matches!(
                            record.address() as usize,
                            bao1x_api::offsets::SWAP_START_UF2..=SWAP_END_ADDR
                        ) {
                            ("swap", SWAP_BYTES.fetch_add(record.data().len() as u32, Ordering::SeqCst))
                        } else {
                            ("none", 0)
                        }
                    };
                    if status != 0 && status % UX_UPDATE_INTERVAL_BYTES == 0 {
                        if IS_BAOSEC.load(Ordering::SeqCst) {
                            // conjure a pointer to the sh1107 object
                            let iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
                            let (channel, _, _, _) = bao1x_hal::board::get_display_pins();
                            // these parameters are copied out of the sh1107 driver. Maybe we should just
                            // create a convenience function that "just sets
                            // these" since hardware peripherals don't
                            // spontaneously move around, and when they do you'd like to have a single spot to
                            // maintain the changes...
                            let mut sh1107 = unsafe {
                                Oled128x128::from_raw_parts(
                                    (
                                        match channel {
                                            SpimChannel::Channel0 => utra::udma_spim_0::HW_UDMA_SPIM_0_BASE,
                                            SpimChannel::Channel1 => utra::udma_spim_1::HW_UDMA_SPIM_1_BASE,
                                            SpimChannel::Channel2 => utra::udma_spim_2::HW_UDMA_SPIM_2_BASE,
                                            SpimChannel::Channel3 => utra::udma_spim_3::HW_UDMA_SPIM_3_BASE,
                                        },
                                        SpimCs::Cs0,
                                        0,
                                        0,
                                        None,
                                        SpimMode::Standard,
                                        SpimByteAlign::Disable,
                                        bao1x_hal::ifram::IframRange::from_raw_parts(
                                            bao1x_hal::board::DISPLAY_IFRAM_ADDR,
                                            bao1x_hal::board::DISPLAY_IFRAM_ADDR,
                                            4096 * 2,
                                        ),
                                        2048 + 256,
                                        2048,
                                        0,
                                    ),
                                    &iox,
                                )
                            };
                            // have to restore this because the frame buffer is lost on the raw-parts
                            // conversion
                            sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
                            let msg = alloc::format!("{} - {}k", partition, status / 1024);
                            crate::marquee(&mut sh1107, &msg);
                        } else {
                            crate::println_d!("{} - {}k", partition, status / 1024);
                        }
                    }
                }
                // replace the tracking block with the new one, if a new one was provided
                if let Some(block) = new_block {
                    critical_section::with(|cs| super::glue::SECTOR.borrow(cs).replace(block));
                }
            }
            if write_offset + len < disk.len() {
                // update the received data to the disk if it fits within the allocated region
                disk[write_offset..write_offset + len].copy_from_slice(&app_buf[..len]);
            }
            if let Some((offset, remaining_len)) = this.remaining_wr.take() {
                this.setup_big_write(app_buf_addr(), app_buf_len(), offset, remaining_len);
                this.ms_state = UmsState::DataPhase;
                csw.update_hw();
            } else {
                csw.residue = 0;
                csw.status = 0;
                csw.send(this);
                crate::DISK_BUSY.store(false, Ordering::SeqCst);
            }
        } else {
            crate::println_d!("Data completion reached without destination for data copy! data dropped.");
        }
    } else {
        crate::println_d!("uhhh wtf");
    }
}

pub fn usb_ep1_bulk_in_complete(
    this: &mut CorigineUsb,
    _buf_addr: usize,
    info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println_d!("bulk IN handler");
    let length = info & 0xFFFF;
    if UmsState::DataPhase == this.ms_state {
        //DATA
        if let Some((offset, len)) = this.remaining_rd.take() {
            let app_buf = conjure_app_buf();
            let disk = conjure_disk();
            this.setup_big_read(app_buf, disk, offset, len, Some(fill_sparse_data));
            this.ms_state = UmsState::DataPhase;
        } else {
            this.bulk_xfer(1, USB_SEND, CSW_ADDR, 13, 0, 0);
            this.ms_state = UmsState::StatusPhase;
            crate::DISK_BUSY.store(false, Ordering::SeqCst);
        }
    } else if UmsState::StatusPhase == this.ms_state && length == 13 {
        //CSW
        this.bulk_xfer(1, USB_RECV, CBW_ADDR, 31, 0, 0);
        this.ms_state = UmsState::CommandPhase;
    }
}

// ===== CDC Bulk OUT (EP3 OUT) =====
pub fn usb_ep3_bulk_out_complete(
    this: &mut CorigineUsb,
    buf_addr: usize,
    _info: u32,
    _error: u8,
    residual: u16,
) {
    // crate::println_d!("EP3 OUT: {:x} {:x}", info, _error);

    let actual = CRG_UDC_APP_BUF_LEN - residual as usize;

    if actual == 0 {
        return; // zero-length packet, ignore
    }

    // Slice of received data
    let buf = unsafe { core::slice::from_raw_parts(buf_addr as *const u8, actual as usize) };

    // For now: just print, or push into a ring buffer for your "virtual terminal"
    // crate::println_d!("CDC OUT received {} bytes: {:?}", actual, &buf);

    critical_section::with(|cs| {
        let mut queue = crate::USB_RX.borrow(cs).borrow_mut();
        for &d in buf {
            queue.push_back(d);
        }
    });

    // Re-arm OUT transfer so host can send more
    let acm_buf = this.cdc_acm_rx_slice();
    this.bulk_xfer(3, USB_RECV, acm_buf.as_ptr() as usize, acm_buf.len(), 0, 0);
}

pub fn flush() {
    unsafe {
        if let Some(ref mut usb_ref) = crate::platform::usb::USB {
            let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
            flush_tx(usb);
        }
    }
}

pub fn flush_tx(this: &mut CorigineUsb) {
    const TIMEOUT: usize = 100_000;
    let mut written = 0;

    let mut timeout = 0;
    while !crate::platform::usb::TX_IDLE.swap(false, core::sync::atomic::Ordering::SeqCst)
        && timeout < TIMEOUT
    {
        timeout += 1;
        if timeout % 20_000 == 0 {
            // suppress this unless actively debugging because this can pollute the
            // tx queue with data and eventually cause an overflow
            // crate::println!("txw {}", timeout);
        }
    }

    let tx_buf = this.cdc_acm_tx_slice();
    critical_section::with(|cs| {
        let mut queue = crate::USB_TX.borrow(cs).borrow_mut();
        let to_copy = queue.len().min(CRG_UDC_APP_BUF_LEN);

        let (a, b) = queue.as_slices();
        if to_copy <= a.len() {
            tx_buf[..to_copy].copy_from_slice(&a[..to_copy]);
            queue.drain(..to_copy);
            written = to_copy;
        } else {
            let first = a.len();
            let second = to_copy - first;
            tx_buf[..first].copy_from_slice(a);
            tx_buf[first..to_copy].copy_from_slice(&b[..second]);
            queue.drain(..to_copy);
            written = to_copy;
        }
    });

    if written > 0 {
        this.bulk_xfer(3, USB_SEND, tx_buf.as_ptr() as usize, written, 0, 0);
    } else {
        // release the lock
        TX_IDLE.store(true, Ordering::SeqCst);
    }
}

// ===== CDC Bulk IN (EP3 IN) =====
pub fn usb_ep3_bulk_in_complete(
    this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println_d!("EP3 IN");
    // let length = CRG_UDC_APP_BUF_LEN - _residual as usize;
    // crate::println_d!("CDC IN transfer complete, {} bytes sent", length);

    // signal that more stuff can be put into the pipe
    TX_IDLE.store(true, Ordering::SeqCst);

    // this may or may not initiate a new connection, depending on how full the Tx buffer is
    flush_tx(this);
}

// ===== CDC Notification IN (EP2 IN) =====
pub fn usb_ep2_int_in_complete(
    _this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println_d!("EP2 INT");
    // let length = CRG_UDC_APP_BUF_LEN - residual as usize;
    // crate::println_d!("CDC notification sent, {} bytes", length);

    // Typically sends SERIAL_STATE bitmap (carrier detect etc).
    // Ignoring - this is a virtual terminal
}

fn process_mass_storage_command(this: &mut CorigineUsb, cbw: Cbw) {
    match cbw.cdb[0] {
      0x00 => { /* Request the device to report if it is ready */
        process_test_unit_ready(this, cbw);
      }
      0x03 => { /* Transfer status sense data to the host */
        process_request_sense(this, cbw);
      }
      0x12 => { /* Inquity command. Get device information */

        process_inquiry_command(this, cbw);
      }
      0x1E => { /* Prevent or allow the removal of media from a removable
                 ** media device
                 */
        process_prevent_allow_medium_removal(this, cbw);
      }
      0x25 => { /* Report current media capacity */
        process_report_capacity(this, cbw);
      }
      0x9e => {
        process_read_capacity_16(this, cbw);
      }
      0x28 => { /* Read (10) Transfer binary data from media to the host */
        process_read_command(this, cbw);
      }
      0x2A => { /* Write (10) Transfer binary data from the host to the
                 ** media
                 */
        process_write_command(this, cbw);
      }
      0xAA => { /* Write (12) Transfer binary data from the host to the
                 ** media
                 */
         process_write12_command(this, cbw);
      }
      0x01 | /* Position a head of the drive to zero track */
      0x04 | /* Format unformatted media */
      0x1A |
      0x1B | /* Request a request a removable-media device to load or
                 ** unload its media
                 */
      0x1D | /* Perform a hard reset and execute diagnostics */
      0x23 | /* Read Format Capacities. Report current media capacity and
                 ** formattable capacities supported by media
                 */
      0x2B | /* Seek the device to a specified address */
      0x2E | /* Transfer binary data from the host to the media and
                 ** verify data
                 */
      0x2F | /* Verify data on the media */
      0x55 | /* Allow the host to set parameters in a peripheral */
      0x5A | /* Report parameters to the host */
      0xA8 /* Read (12) Transfer binary data from the media to the host */ => {
        process_unsupported_command(this, cbw);
      }
      _ => {
        process_unsupported_command(this, cbw);
     }
    }
}

fn process_test_unit_ready(this: &mut CorigineUsb, _cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;

    csw.send(this);
}

fn process_request_sense(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 0;
        csw.send(this);
        // crate::println_d!("UMS_STATE_STATUS_PHASE\r\n");
    } else if cbw.flags & 0x80 != 0 {
        if cbw.data_transfer_length < 18 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..18].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, cbw.data_transfer_length as usize, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = 0;
            csw.status = 0;
        } else if cbw.data_transfer_length >= 18 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..18].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 18, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = cbw.data_transfer_length - 18;
            csw.status = 0;
        }
    }
    csw.update_hw();
}

fn process_inquiry_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    let inquiry_data = InquiryResponse {
        peripheral_device_type: 0,
        rmb: 0,
        version: 0,
        response_data_format: 1,
        additional_length: 31,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        vendor_identification: *b"Bao Semi",
        product_identification: *b"USB update vdisk",
        product_revision_level: *b"demo",
    };

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 0;
        csw.send(this);
    } else if cbw.flags & 0x80 != 0 {
        if cbw.data_transfer_length < 36 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..36].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, cbw.data_transfer_length as usize, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = 0;
            csw.status = 0;
        } else if cbw.data_transfer_length as usize >= size_of::<InquiryResponse>() {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..size_of::<InquiryResponse>()].copy_from_slice(inquiry_data.as_ref());

            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, size_of::<InquiryResponse>(), 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = cbw.data_transfer_length - size_of::<InquiryResponse>() as u32;
            csw.status = 0;
        }
    }
    csw.update_hw();
}

fn process_prevent_allow_medium_removal(this: &mut CorigineUsb, _cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;
    csw.send(this);
}

fn process_report_capacity(this: &mut CorigineUsb, cbw: Cbw) {
    crate::println_d!("REPORT CAPACITY");
    let mut csw = Csw::derive();
    let rc_lba = RAMDISK_LEN / SECTOR_SIZE as usize - 1;
    let rc_bl: u32 = SECTOR_SIZE as u32;
    let mut capacity = [0u8; 8];

    capacity[..4].copy_from_slice(&rc_lba.to_be_bytes());
    capacity[4..].copy_from_slice(&rc_bl.to_be_bytes());

    if cbw.flags & 0x80 != 0 {
        let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
        ep1_in[..8].fill(0);
        ep1_in[..8].copy_from_slice(&capacity);
        this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 8, 0, 0);
        this.ms_state = UmsState::DataPhase;
    } else {
        csw.send(this);
    }
    csw.residue = 0;
    csw.status = 0;
    csw.update_hw();
}

fn process_read_capacity_16(this: &mut CorigineUsb, _cbw: Cbw) {
    let rc_lba = (RAMDISK_LEN / SECTOR_SIZE as usize - 1) as u64;
    let rc_bl: u32 = SECTOR_SIZE as u32;

    let mut response = [0u8; 32];
    response[..8].copy_from_slice(&rc_lba.to_be_bytes());
    response[8..12].copy_from_slice(&rc_bl.to_be_bytes());
    let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
    ep1_in[..32].copy_from_slice(&response);
    this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 32, 0, 0);
    this.ms_state = UmsState::DataPhase;
}

fn process_unsupported_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;

    if cbw.flags & 0x80 != 0 {
        this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 0, 0, 0);
        this.ms_state = UmsState::DataPhase;
    } else {
        csw.send(this);
    }
    csw.update_hw();
}

fn process_read_command(this: &mut CorigineUsb, cbw: Cbw) {
    crate::DISK_BUSY.store(true, Ordering::SeqCst);
    let mut csw = Csw::derive();
    let mut lba;
    let mut length;

    lba = (cbw.cdb[4] as u32) << 8;
    lba |= cbw.cdb[5] as u32;
    length = (cbw.cdb[7] as u32) << 8;
    length |= cbw.cdb[8] as u32;

    length *= SECTOR_SIZE as u32;

    if cbw.flags & 0x80 == 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
        return;
    }
    crate::println_d!(
        "DISK READ @ 0x{:x} .. 0x{:x}",
        RAMDISK_ADDRESS + lba as usize * SECTOR_SIZE as usize,
        length
    );
    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;

        csw.send(this);
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length < length {
            length = cbw.data_transfer_length;
            csw.residue = cbw.data_transfer_length;
            csw.status = 1;
        } else if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            csw.status = 1;
        }
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(
            app_buf,
            disk,
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
            Some(fill_sparse_data),
        );
        this.ms_state = UmsState::DataPhase;
    }
}

fn process_write_command(this: &mut CorigineUsb, cbw: Cbw) {
    crate::DISK_BUSY.store(true, Ordering::SeqCst);
    let mut csw = Csw::derive();

    let mut lba: u32;
    let mut length: u32;

    lba = (cbw.cdb[4] as u32) << 8;
    lba |= cbw.cdb[5] as u32;
    length = (cbw.cdb[7] as u32) << 8;
    length |= cbw.cdb[8] as u32;

    length *= SECTOR_SIZE as u32;

    if cbw.flags & 0x80 != 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(
            app_buf,
            disk,
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
            Some(fill_sparse_data),
        );
        this.ms_state = UmsState::DataPhase;
        csw.update_hw();
        return;
    }

    crate::println_d!(
        "DISK WRITE @ 0x{:x} .. 0x{:x}",
        RAMDISK_ADDRESS + lba as usize * SECTOR_SIZE as usize,
        length
    );

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;
        csw.send(this);
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            length = cbw.data_transfer_length;
            csw.status = 1;
        } else if cbw.data_transfer_length < length {
            csw.residue = cbw.data_transfer_length;
            csw.status = 1;
            length = cbw.data_transfer_length;
        }
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
    }
    csw.update_hw();
}

fn process_write12_command(this: &mut CorigineUsb, cbw: Cbw) {
    crate::DISK_BUSY.store(true, Ordering::SeqCst);
    let mut csw = Csw::derive();

    let mut lba;
    let mut length;

    lba = (cbw.cdb[2] as u32) << 24;
    lba |= (cbw.cdb[3] as u32) << 16;
    lba |= (cbw.cdb[4] as u32) << 8;
    lba |= (cbw.cdb[5] as u32) << 0;
    length = (cbw.cdb[6] as u32) << 24;
    length |= (cbw.cdb[7] as u32) << 16;
    length |= (cbw.cdb[8] as u32) << 8;
    length |= (cbw.cdb[9] as u32) << 0;

    length *= SECTOR_SIZE as u32;
    crate::println_d!("write12 of {} bytes", length);

    if cbw.flags & 0x80 != 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        // note: zero-length transfer but we still update the buffer because that's what the reference driver
        // does
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(
            app_buf,
            disk,
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
            Some(fill_sparse_data),
        );
        this.ms_state = UmsState::DataPhase;
        csw.update_hw();
        return;
    }

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;
        csw.send(this);
        return;
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            length = cbw.data_transfer_length;
        } else if cbw.data_transfer_length < length {
            csw.residue = cbw.data_transfer_length;
            csw.status = 2;
            length = cbw.data_transfer_length;
        }
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
    }
    csw.update_hw();
}

// Place the application buf in IFRAM1 range - this removes any conflicts with the USB stack's IFRAM0
// allocations
pub(crate) fn app_buf_addr() -> usize { HW_IFRAM1_MEM }
pub(crate) fn app_buf_len() -> usize { 4096 * 2 }
pub(crate) fn conjure_app_buf() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(app_buf_addr() as *mut u8, app_buf_len()) }
}

pub(crate) fn conjure_disk() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(RAMDISK_ADDRESS as *mut u8, RAMDISK_ACTUAL_LEN) }
}
