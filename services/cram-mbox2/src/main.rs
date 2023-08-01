use utralib::generated::*;
use num_traits::*;

pub struct MailboxClient {
    csr: utralib::CSR<u32>,
    // the IRQ handler may or may not be at a different address depending on the configuration
    // Loopback is at the same CSR (in which case the underlying pointer is the same as above)
    // External source is at a different address.
    csr_irq: utralib::CSR<u32>,
    cid: xous::CID,
    abort_pending: bool,
}

impl MailboxClient {
    pub fn send(&mut self, data: &[u32]) {
        // defer aborts until this interaction is done
        self.csr_irq.wo(utra::mb_client::EV_ENABLE,
            self.csr_irq.r(utra::mb_client::EV_ENABLE) &
            !self.csr_irq.ms(utra::mb_client::EV_ENABLE_ABORT_INIT, 1)
        );
        // interact with the FIFO
        for &d in data {
            while self.csr.rf(utra::mb_client::STATUS_TX_FREE) == 0 {
                // busy-wait
            }
            self.csr.wfo(utra::mb_client::WDATA_WDATA, d);
        }
        self.csr.wfo(utra::mb_client::DONE_DONE, 1);
        // re-enable aborts
        self.csr_irq.wo(utra::mb_client::EV_ENABLE,
            self.csr_irq.r(utra::mb_client::EV_ENABLE) |
            self.csr_irq.ms(utra::mb_client::EV_ENABLE_ABORT_INIT, 1)
        );
    }
    pub fn get(&mut self, ret: &mut [u32]) -> usize {
        // defer aborts until this interaction is done
        self.csr_irq.wo(utra::mb_client::EV_ENABLE,
            self.csr_irq.r(utra::mb_client::EV_ENABLE) &
            !self.csr_irq.ms(utra::mb_client::EV_ENABLE_ABORT_INIT, 1)
        );
        // interact with the FIFO
        // note: this only works because rx_words is the LSB of the register. We don't have to shift the MS'd value.
        while self.csr.rf(utra::mb_client::STATUS_RX_AVAIL) == 0 {
            // wait for data to be available
        }
        let test_definition = self.csr.r(utra::mb_client::RDATA);
        let rx_words = (test_definition >> 16) as usize;
        if rx_words > ret.len() {
            log::warn!("rx_words is too large: {} vs {}", rx_words, ret.len());
            // this will lead to a test failure, but this is handled by the tester driver...mostly...
        }
        ret[0] = test_definition;
        if rx_words > 1 {
            let limit = rx_words.min(ret.len());
            for d in ret[1..limit].iter_mut() {
                while self.csr.rf(utra::mb_client::STATUS_RX_AVAIL) == 0 {
                    // wait for data to be available
                }
                *d = self.csr.r(utra::mb_client::RDATA);
            }
        }
        // re-enable aborts
        self.csr_irq.wo(utra::mb_client::EV_ENABLE,
            self.csr_irq.r(utra::mb_client::EV_ENABLE) |
            self.csr_irq.ms(utra::mb_client::EV_ENABLE_ABORT_INIT, 1)
        );
        rx_words
    }
    pub fn abort(&mut self) {
        log::warn!("abort initiated");
        self.csr.wfo(utra::mb_client::CONTROL_ABORT, 1);
    }
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
    Incoming,
    AbortInit,
    AbortDone,
    ProtocolError,
    InvalidCall,
}

fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let mb_client = unsafe { &mut *(arg as *mut MailboxClient) };

    let pending = mb_client.csr_irq.r(utra::mb_client::EV_PENDING);
    if pending & mb_client.csr_irq.ms(utra::mb_client::EV_PENDING_ERROR, 1) != 0 {
        xous::try_send_message(mb_client.cid, xous::Message::new_scalar(
            Opcode::ProtocolError.to_usize().unwrap(), pending as usize, 0, 0, 0)
        ).ok();
    }
    if pending & mb_client.csr_irq.ms(utra::mb_client::EV_PENDING_ABORT_INIT, 1) != 0 {
        mb_client.abort_pending = true;
        xous::try_send_message(mb_client.cid, xous::Message::new_scalar(
            Opcode::AbortInit.to_usize().unwrap(), pending as usize, 0, 0, 0)
        ).ok();
    }
    if pending & mb_client.csr_irq.ms(utra::mb_client::EV_PENDING_ABORT_DONE, 1) != 0 {
        xous::try_send_message(mb_client.cid, xous::Message::new_scalar(
            Opcode::AbortDone.to_usize().unwrap(), pending as usize, 0, 0, 0)
        ).ok();
    }
    if pending & mb_client.csr_irq.ms(utra::mb_client::EV_PENDING_AVAILABLE, 1) != 0 {
        xous::try_send_message(mb_client.cid, xous::Message::new_scalar(
            Opcode::Incoming.to_usize().unwrap(), pending as usize, 0, 0, 0)
        ).ok();
    }

    mb_client.csr_irq
        .wo(utra::mb_client::EV_PENDING, mb_client.csr_irq.r(utra::mb_client::EV_PENDING));
}

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let xns = xous_api_names::XousNames::new().unwrap();
    let client_sid = xns.register_name("_mbox_client_", None).expect("can't register server");
    let client_cid = xous::connect(client_sid).unwrap();

    let mb_client_csr = xous::syscall::map_memory(
        #[cfg(not(feature="ext"))]
        xous::MemoryAddress::new(utra::mb_client::HW_MB_CLIENT_BASE),
        #[cfg(feature="ext")]
        xous::MemoryAddress::new(utra::mb_ext::HW_MB_EXT_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map mbox client CSR range");
    let mb_client = CSR::new(mb_client_csr.as_mut_ptr() as *mut u32);
    #[cfg(feature="ext")]
    let mb_client_irq_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::irqarray3::HW_IRQARRAY3_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map ext interrupt CSR range");

    let mut mb_client = MailboxClient {
        csr: mb_client,
        #[cfg(feature="ext")]
        csr_irq: CSR::new(mb_client_irq_csr.as_mut_ptr() as *mut u32),
        #[cfg(not(feature="ext"))]
        csr_irq: CSR::new(mb_client_csr.as_mut_ptr() as *mut u32),
        cid: client_cid,
        abort_pending: false,
    };
    xous::claim_interrupt(
        #[cfg(not(feature="ext"))]
        utra::mb_client::MB_CLIENT_IRQ,
        #[cfg(feature="ext")]
        utra::irqarray3::IRQARRAY3_IRQ,
        handle_irq,
        (&mut mb_client) as *mut MailboxClient as *mut usize,
    )
    .expect("couldn't claim irq");
    #[cfg(feature="ext")]
    mb_client.csr_irq.wo(utra::irqarray3::EV_EDGE_TRIGGERED, 0b1110); // filter for rising edges on these bits
    #[cfg(feature="ext")]
    mb_client.csr_irq.wo(utra::irqarray3::EV_POLARITY, 0b1110); // rising edge

    mb_client.csr_irq.wo(utra::mb_client::EV_ENABLE, 0b1111); // enable everything

    let mut msg_opt = None;
    let mut return_type = 0;
    let mut test_data = [0u32; 1024];

    loop {
        xous::reply_and_receive_next_legacy(client_sid, &mut msg_opt, &mut return_type)
            .unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let op = num_traits::FromPrimitive::from_usize(msg.body.id())
            .unwrap_or(Opcode::InvalidCall);
        log::info!("Got {:?}", op);
        match op
        {
            Opcode::Incoming => {
                if mb_client.abort_pending {
                    log::info!("Got abort in between rx IRQ and rx handler");
                    // ignore the packet, let the abort handler run
                    continue;
                }
                if let Some(_scalar) = msg.body.scalar_message() {
                    let len = mb_client.get(&mut test_data);
                    for d in test_data[..len].iter_mut() {
                        *d = *d ^ 0xAAAA_0000;
                    }
                    let test_seq = test_data[0] & 0xFFFF;
                    log::info!("rx seq {}, {} words", test_seq, len);
                    if test_seq == 8 { // abort on case #8
                        mb_client.abort();
                    } else {
                        mb_client.send(&test_data[..len]);
                    }
                } else {
                    log::error!("Wrong message type for RunTest");
                }
            }
            Opcode::AbortInit => {
                log::info!("test peer initiated abort!");
                mb_client.abort_pending = false;
                mb_client.csr.wfo(utra::mb_client::CONTROL_ABORT, 1);
            }
            Opcode::AbortDone => {
                mb_client.abort_pending = false;
                log::info!("client abort protocol done");
            }
            Opcode::ProtocolError => {
                if let Some(scalar) = msg.body.scalar_message() {
                    log::error!("Protocol error received: {:x}, aborting test", scalar.arg1);
                    break;
                } else {
                    log::error!("Wrong message type for ProtocolError; aborting test");
                    break;
                }
            }
            Opcode::InvalidCall => {
                log::error!("Invalid opcode: {:?}", msg);
            }
        }

    }
}
