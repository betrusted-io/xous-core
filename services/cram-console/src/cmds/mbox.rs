use modals::Modals;
use utra::mailbox;
use utralib::generated::*;
use String;

use crate::{CommonEnv, ShellCmdApi};

/// This constraint is limited by the size of the memory on the CM7 side
const MAX_PKT_LEN: usize = 128;
const MBOX_PROTOCOL_REV: u32 = 0;
const TX_FIFO_DEPTH: u32 = 128;

const CLIFFORD_SIZE: usize = 128;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum MboxError {
    None,
    NotReady,
    TxOverflow,
    TxUnderflow,
    RxOverflow,
    RxUnderflow,
    InvalidOpcode,
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToRvOp {
    Invalid = 0,

    RetKnock = 128,
    RetDct8x8 = 129,
    RetClifford = 130,
}
impl TryFrom<u16> for ToRvOp {
    type Error = MboxError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ToRvOp::Invalid),
            128 => Ok(ToRvOp::RetKnock),
            129 => Ok(ToRvOp::RetDct8x8),
            130 => Ok(ToRvOp::RetClifford),
            _ => Err(MboxError::InvalidOpcode),
        }
    }
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ToCm7Op {
    Invalid = 0,

    Knock = 1,
    Dct8x8 = 2,
    Clifford = 3,
}

pub struct MboxToCm7Pkt<'a> {
    version: u32,
    opcode: ToCm7Op,
    data: &'a [u32],
}

pub struct MboxToRvPkt {
    version: u32,
    opcode: ToRvOp,
    data: Vec<u32>,
}

pub struct Mbox {
    csr: CSR<u32>,
    phys_mem: xous::MemoryRange,
    modals: Modals,
}
impl Mbox {
    pub fn new() -> Mbox {
        let swapper = xous_swapper::Swapper::new().expect("couldn't get handle to swapper");
        let phys_mem;
        let pages_to_alloc = (CLIFFORD_SIZE * CLIFFORD_SIZE + (4096 - 1)) / 4096;
        loop {
            swapper.garbage_collect_pages(pages_to_alloc);
            match xous::syscall::map_memory(
                None,
                None,
                pages_to_alloc * 4096,
                xous::MemoryFlags::R
                    | xous::MemoryFlags::W
                    | xous::MemoryFlags::RESERVE
                    | xous::MemoryFlags::DEV,
            ) {
                Ok(range) => {
                    phys_mem = Some(range);
                    break;
                }
                Err(e) => {
                    log::info!("Couldn't allocate contiguous memory, retrying: {:?}", e);
                    xous::yield_slice();
                }
            }
        }
        log::info!("Device RAM allocated at vaddr {:x}", phys_mem.unwrap().as_ptr() as usize);
        unsafe {
            phys_mem.unwrap().as_slice_mut().fill(0);
        }
        log::info!("Range is zeroized");

        let csr_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(mailbox::HW_MAILBOX_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map mailbox CSR range");
        let mut csr = CSR::new(csr_mem.as_mut_ptr() as *mut u32);
        csr.wfo(mailbox::LOOPBACK_LOOPBACK, 0); // ensure we're not in loopback mode
        // generate available events - not hooked up to IRQ, but we'll poll for now
        csr.wfo(mailbox::EV_ENABLE_AVAILABLE, 1);

        let xns = xous_names::XousNames::new().unwrap();

        Self { csr, phys_mem: phys_mem.unwrap(), modals: Modals::new(&xns).unwrap() }
    }

    fn expect_tx(&mut self, val: u32) -> Result<(), MboxError> {
        if (TX_FIFO_DEPTH - self.csr.rf(mailbox::STATUS_TX_WORDS)) == 0 {
            return Err(MboxError::TxOverflow);
        } else {
            self.csr.wo(mailbox::WDATA, val);
            Ok(())
        }
    }

    pub fn try_send(&mut self, to_cm7: MboxToCm7Pkt) -> Result<(), MboxError> {
        if to_cm7.data.len() > MAX_PKT_LEN {
            Err(MboxError::TxOverflow)
        } else {
            self.expect_tx(to_cm7.version)?;
            self.expect_tx(to_cm7.opcode as u32 | (to_cm7.data.len() as u32) << 16)?;
            for &d in to_cm7.data.iter() {
                self.expect_tx(d)?;
            }
            // trigger the send
            self.csr.wfo(mailbox::DONE_DONE, 1);
            Ok(())
        }
    }

    fn expect_rx(&mut self) -> Result<u32, MboxError> {
        if self.csr.rf(mailbox::STATUS_RX_WORDS) == 0 {
            Err(MboxError::RxUnderflow)
        } else {
            Ok(self.csr.r(mailbox::RDATA))
        }
    }

    pub fn try_rx(&mut self) -> Result<MboxToRvPkt, MboxError> {
        let version = self.expect_rx()?;
        let op_and_len = self.expect_rx()?;
        let opcode = ToRvOp::try_from((op_and_len & 0xFFFF) as u16)?;
        let len = (op_and_len >> 16) as usize;
        let mut data = Vec::new();
        for _ in 0..len {
            data.push(self.expect_rx()?);
        }
        Ok(MboxToRvPkt { version, opcode, data })
    }

    pub fn poll_not_ready(&self) -> bool { self.csr.rf(mailbox::EV_PENDING_AVAILABLE) == 0 }
}

impl<'a> ShellCmdApi<'a> for Mbox {
    cmd_api!(mbox);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "mbox [knock]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "knock" => {
                    let test_data = [0xC0DE_0000u32, 0x0000_600Du32];
                    let mut expected_result = 0;
                    for &d in test_data.iter() {
                        expected_result ^= d;
                    }
                    let test_pkt =
                        MboxToCm7Pkt { version: MBOX_PROTOCOL_REV, opcode: ToCm7Op::Knock, data: &test_data };
                    log::info!("sending knock...");
                    match self.try_send(test_pkt) {
                        Ok(_) => {
                            write!(ret, "Packet send Ok\n").ok();
                            let start = env.ticktimer.elapsed_ms();
                            while self.poll_not_ready() {
                                xous::yield_slice();
                                if env.ticktimer.elapsed_ms() - start > 1000 {
                                    write!(ret, "Response timeout!\n").ok();
                                    return Ok(Some(ret));
                                }
                            }
                            // now receive the packet
                            match self.try_rx() {
                                Ok(rx_pkt) => {
                                    log::info!("Knock result: {:x}", rx_pkt.data[0]);
                                    if rx_pkt.version != MBOX_PROTOCOL_REV {
                                        write!(
                                            ret,
                                            "Version mismatch {} != {}",
                                            rx_pkt.version, MBOX_PROTOCOL_REV
                                        )
                                        .ok();
                                    }
                                    if rx_pkt.opcode != ToRvOp::RetKnock {
                                        write!(
                                            ret,
                                            "Opcode mismatch {} != {}",
                                            rx_pkt.opcode as u16,
                                            ToRvOp::RetKnock as u16
                                        )
                                        .ok();
                                    }
                                    if rx_pkt.data.len() != 1 {
                                        write!(
                                            ret,
                                            "Expected length mismatch {} != {}",
                                            rx_pkt.data.len(),
                                            1
                                        )
                                        .ok();
                                    } else {
                                        if rx_pkt.data[0] != expected_result {
                                            write!(
                                                ret,
                                                "Expected data mismatch {:x} != {:x}",
                                                rx_pkt.data[0], expected_result
                                            )
                                            .ok();
                                        } else {
                                            write!(ret, "Knock test PASS: {:x}", rx_pkt.data[0]).ok();
                                        }
                                    }
                                }
                                Err(e) => {
                                    write!(ret, "Error while deserializing: {:?}\n", e).ok();
                                }
                            }
                        }
                        Err(e) => {
                            write!(ret, "Packet send error: {:?}\n", e).ok();
                        }
                    };
                }
                // Time trial results
                //   - on Precursor (100MHz): 27.60 seconds
                //   - RV32 Daric (400MHz): 10.11 seconds (2.72x over Precursor)
                //   - CM7 Daric (400MHz): 3.66 seconds (2.76x over RV32; 7.54x over Precursor)
                "clifford" => {
                    log::info!("prefill");
                    let prefill = unsafe { self.phys_mem.as_slice_mut() };
                    log::info!("prefill: {:x}({})", prefill.as_ptr() as usize, prefill.len());
                    prefill.fill(0);
                    log::info!("v2p");
                    let virt_addr = self.phys_mem.as_ptr();
                    let phys_addr =
                        xous::syscall::virt_to_phys(virt_addr as usize).expect("can't convert v2p");
                    log::info!("phys addr: {:x}", phys_addr);
                    let test_data = [phys_addr as u32];
                    let clifford_pkt = MboxToCm7Pkt {
                        version: MBOX_PROTOCOL_REV,
                        opcode: ToCm7Op::Clifford,
                        data: &test_data,
                    };
                    log::info!("initiating attractor");
                    let start_time = env.ticktimer.elapsed_ms();
                    let mut end_time: Option<u64> = None;
                    match self.try_send(clifford_pkt) {
                        Ok(_) => {
                            write!(ret, "Packet send Ok\n").ok();
                            #[cfg(feature = "clifford-poll")]
                            {
                                let start = env.ticktimer.elapsed_ms();
                                while self.poll_not_ready() {
                                    xous::yield_slice();
                                    if env.ticktimer.elapsed_ms() - start > 10_000 {
                                        write!(ret, "Response timeout!\n").ok();
                                        return Ok(Some(ret));
                                    }
                                }
                                // now receive the packet
                                log::info!("computation done, rendering...");
                                match self.try_rx() {
                                    Ok(_) => {
                                        // get the computed data and show it
                                        // safety: safe because all u8 types are representable
                                        let buf: &[u8] = &unsafe { self.phys_mem.as_slice() }
                                            [..CLIFFORD_SIZE * CLIFFORD_SIZE];
                                        let img = gam::bitmap::Img::new(
                                            buf.to_owned(),
                                            CLIFFORD_SIZE,
                                            gam::PixelType::U8,
                                        );
                                        log::info!("showing image...");
                                        let modal_size =
                                            gam::Point::new(CLIFFORD_SIZE as _, CLIFFORD_SIZE as _);
                                        let bm = gam::Bitmap::from_img(&img, Some(modal_size));
                                        self.modals.show_image(bm).expect("couldn't render attractor");
                                        log::info!("done!");
                                    }
                                    Err(e) => {
                                        write!(ret, "Error while deserializing: {:?}\n", e).ok();
                                    }
                                }
                            }
                            #[cfg(not(feature = "clifford-poll"))]
                            {
                                const WIDTH: u32 = CLIFFORD_SIZE as _;
                                const HEIGHT: u32 = CLIFFORD_SIZE as _;
                                const X_CENTER: f32 = (WIDTH / 2) as f32;
                                const Y_CENTER: f32 = (HEIGHT / 2) as f32;
                                const SCALE: f32 = WIDTH as f32 / 5.1;
                                const STEP: u8 = 16;
                                const ITERATIONS: u32 = 200000;
                                let mut buf = vec![255u8; (WIDTH * HEIGHT).try_into().unwrap()];
                                let (a, b, c, d) = (-2.0, -2.4, 1.1, -0.9);
                                let (mut x, mut y): (f32, f32) = (0.0, 0.0);

                                log::info!("generating image");
                                for _ in 0..=ITERATIONS {
                                    if end_time.is_none() && !self.poll_not_ready() {
                                        end_time = Some(env.ticktimer.elapsed_ms());
                                    }
                                    // this takes a couple minutes to run
                                    let x1 = f32::sin(a * y) + c * f32::cos(a * x);
                                    let y1 = f32::sin(b * x) + d * f32::cos(b * y);
                                    (x, y) = (x1, y1);
                                    let (a, b): (u32, u32) =
                                        ((x * SCALE + X_CENTER) as u32, (y * SCALE + Y_CENTER) as u32);
                                    let i: usize = (a + WIDTH * b).try_into().unwrap();
                                    if buf[i] >= STEP {
                                        buf[i] -= STEP;
                                    }
                                }
                                log::info!(
                                    "Local finished in {:.2} s",
                                    (env.ticktimer.elapsed_ms() - start_time) as f32 / 1000.0
                                );
                                while end_time.is_none() {
                                    if !self.poll_not_ready() {
                                        end_time = Some(env.ticktimer.elapsed_ms());
                                        break;
                                    }
                                }
                                log::info!(
                                    "Remote finished in {:.2} s",
                                    (end_time.unwrap_or(0) - start_time) as f32 / 1000.0
                                );
                                let img = gam::Img::new(buf, WIDTH.try_into().unwrap(), gam::PixelType::U8);
                                log::info!("showing local version");
                                let modal_size = gam::Point::new(CLIFFORD_SIZE as _, CLIFFORD_SIZE as _);
                                let bm = gam::Bitmap::from_img(&img, Some(modal_size));
                                self.modals.show_image(bm).expect("couldn't render attractor");

                                env.ticktimer.sleep_ms(1000).ok();
                                log::info!("showing remote version");
                                // now receive the packet
                                log::info!("computation done, rendering...");
                                match self.try_rx() {
                                    Ok(_) => {
                                        // get the computed data and show it
                                        // safety: safe because all u8 types are representable
                                        let buf: &[u8] = &unsafe { self.phys_mem.as_slice() }
                                            [..CLIFFORD_SIZE * CLIFFORD_SIZE];
                                        let img = gam::bitmap::Img::new(
                                            buf.to_owned(),
                                            CLIFFORD_SIZE,
                                            gam::PixelType::U8,
                                        );
                                        log::info!("showing image...");
                                        let modal_size =
                                            gam::Point::new(CLIFFORD_SIZE as _, CLIFFORD_SIZE as _);
                                        let bm = gam::Bitmap::from_img(&img, Some(modal_size));
                                        self.modals.show_image(bm).expect("couldn't render attractor");
                                        log::info!("done!");
                                    }
                                    Err(e) => {
                                        write!(ret, "Error while deserializing: {:?}\n", e).ok();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            write!(ret, "Packet send error: {:?}\n", e).ok();
                        }
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
