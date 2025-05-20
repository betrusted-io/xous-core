use utra::mailbox;
use utralib::generated::*;

/// This constraint is limited by the size of the memory on the CM7 side
pub const MAX_PKT_LEN: usize = 128;
pub const MBOX_PROTOCOL_REV: u32 = 0;
pub const TX_FIFO_DEPTH: u32 = 128;
pub const PAYLOAD_LEN_WORDS: usize = MAX_PKT_LEN - 2;
pub const RERAM_PAGE_SIZE_BYTES: usize = 32;

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
    ProtocolErr,
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToRvOp {
    Invalid = 0,

    RetKnock = 128,
    RetDct8x8 = 129,
    RetClifford = 130,
    RetFlashWrite = 131,
}
impl TryFrom<u16> for ToRvOp {
    type Error = MboxError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ToRvOp::Invalid),
            128 => Ok(ToRvOp::RetKnock),
            129 => Ok(ToRvOp::RetDct8x8),
            130 => Ok(ToRvOp::RetClifford),
            131 => Ok(ToRvOp::RetFlashWrite),
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
    FlashWrite = 4,
}

pub struct MboxToCm7Pkt<'a> {
    pub version: u32,
    pub opcode: ToCm7Op,
    pub data: &'a [u32],
}

pub struct MboxToRvPkt {
    pub version: u32,
    pub opcode: ToRvOp,
    #[cfg(feature = "std")]
    pub data: Vec<u32>,
    #[cfg(not(feature = "std"))]
    pub len: usize,
}

pub struct Mbox {
    csr: CSR<u32>,
}
impl Mbox {
    pub fn new() -> Mbox {
        #[cfg(feature = "std")]
        let csr_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(mailbox::HW_MAILBOX_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map mailbox CSR range");
        #[cfg(feature = "std")]
        let mut csr = CSR::new(csr_mem.as_mut_ptr() as *mut u32);
        #[cfg(not(feature = "std"))]
        let mut csr = CSR::new(mailbox::HW_MAILBOX_BASE as *mut u32);
        csr.wfo(mailbox::LOOPBACK_LOOPBACK, 0); // ensure we're not in loopback mode
        // generate available events - not hooked up to IRQ, but we'll poll for now
        csr.wfo(mailbox::EV_ENABLE_AVAILABLE, 1);

        Self { csr }
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
        if to_cm7.data.len() > PAYLOAD_LEN_WORDS {
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

    #[cfg(feature = "std")]
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

    #[cfg(not(feature = "std"))]
    pub fn try_rx(&mut self, data: &mut [u32]) -> Result<MboxToRvPkt, MboxError> {
        let version = self.expect_rx()?;
        let op_and_len = self.expect_rx()?;
        let opcode = ToRvOp::try_from((op_and_len & 0xFFFF) as u16)?;
        let len = (op_and_len >> 16) as usize;
        for i in 0..len {
            if i < data.len() {
                data[i] = self.expect_rx()?
            }
        }
        Ok(MboxToRvPkt { version, opcode, len })
    }

    pub fn poll_not_ready(&self) -> bool { self.csr.rf(mailbox::EV_PENDING_AVAILABLE) == 0 }

    pub fn poll_rx_available(&self) -> bool { self.csr.rf(mailbox::STATUS_RX_WORDS) != 0 }
}

// TODO: make these protocol-level interactions more modular. Right now this is just a stand-alone
// test routine suitable for use in a no_std bootloader environment.
//
// TODO: resolve the timeout timer requirement in this code - there isn't a standard timer in
// the bootloader.
/*
#[cfg(not(target_os = "xous"))]
#[allow(dead_code)]
pub fn knock(mbox: &mut Mbox) -> Result<(), MboxError> {
    let test_data = [0xC0DE_0000u32, 0x0000_600Du32];
    let mut expected_result = 0;
    for &d in test_data.iter() {
        expected_result ^= d;
    }
    let test_pkt = MboxToCm7Pkt { version: MBOX_PROTOCOL_REV, opcode: ToCm7Op::Knock, data: &test_data };
    crate::println!("sending knock...");
    match mbox.try_send(test_pkt) {
        Ok(_) => {
            crate::println!("Packet send Ok\n");
            let mut timeout = 0;
            while mbox.poll_not_ready() {
                timeout += 1;
                if timeout > 1000 {
                    crate::println!("flash write timeout");
                    return Err(MboxError::NotReady);
                }
                crate::platform::delay(2);
            } // now receive the packet
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
                    crate::println!("Knock result: {:x}", rx_data[0]);
                    if rx_pkt.version != MBOX_PROTOCOL_REV {
                        crate::println!("Version mismatch {} != {}", rx_pkt.version, MBOX_PROTOCOL_REV);
                    }
                    if rx_pkt.opcode != ToRvOp::RetKnock {
                        crate::println!(
                            "Opcode mismatch {} != {}",
                            rx_pkt.opcode as u16,
                            ToRvOp::RetKnock as u16
                        );
                    }
                    if rx_pkt.len != 1 {
                        crate::println!("Expected length mismatch {} != {}", rx_pkt.len, 1);
                        Err(MboxError::ProtocolErr)
                    } else {
                        if rx_data[0] != expected_result {
                            crate::println!(
                                "Expected data mismatch {:x} != {:x}",
                                rx_data[0],
                                expected_result
                            );
                            Err(MboxError::ProtocolErr)
                        } else {
                            crate::println!("Knock test PASS: {:x}", rx_data[0]);
                            Ok(())
                        }
                    }
                }
                Err(e) => {
                    crate::println!("Error while deserializing: {:?}\n", e);
                    Err(e)
                }
            }
        }
        Err(e) => {
            crate::println!("Packet send error: {:?}\n", e);
            Err(e)
        }
    }
}
*/
