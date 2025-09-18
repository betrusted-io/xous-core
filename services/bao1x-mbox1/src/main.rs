use num_traits::*;
use utralib::generated::*;

pub struct Mailbox {
    csr: utralib::CSR<u32>,
    cid: xous::CID,
    abort_pending: bool,
}
impl Mailbox {
    /// This version of send has no timeouts. A safe version of this would have a time-out
    /// that returns control flow in case of protocol errors.
    ///
    /// Safety: must guarantee that the sender is ready/available to send.
    pub unsafe fn send_unguarded(&mut self, data: &[u32]) {
        let status = self.csr.r(utra::mailbox::STATUS);
        if status & self.csr.ms(utra::mailbox::STATUS_RX_ERR, 1) != 0
            || status & self.csr.ms(utra::mailbox::STATUS_TX_ERR, 1) != 0
        {
            log::warn!("Rx/Tx err was encountered: {:x}", status);
        }
        if status & self.csr.ms(utra::mailbox::STATUS_TX_WORDS, !0) != 0 {
            log::warn!("Tx register is not empty: {:x}", status);
        }
        // defer aborts until this interaction is done
        self.csr.wo(
            utra::mailbox::EV_ENABLE,
            self.csr.r(utra::mailbox::EV_ENABLE) & !self.csr.ms(utra::mailbox::EV_ENABLE_ABORT_INIT, 1),
        );
        // interact with the FIFO
        for &d in data {
            self.csr.wfo(utra::mailbox::WDATA_WDATA, d);
        }
        self.csr.wfo(utra::mailbox::DONE_DONE, 1);
        // re-enable aborts
        self.csr.wo(
            utra::mailbox::EV_ENABLE,
            self.csr.r(utra::mailbox::EV_ENABLE) | self.csr.ms(utra::mailbox::EV_ENABLE_ABORT_INIT, 1),
        );
    }

    pub fn get(&mut self, ret: &mut [u32]) -> usize {
        let mut drain = false;
        let status = self.csr.r(utra::mailbox::STATUS);
        if status & self.csr.ms(utra::mailbox::STATUS_RX_ERR, 1) != 0
            || status & self.csr.ms(utra::mailbox::STATUS_TX_ERR, 1) != 0
        {
            log::warn!("Rx/Tx err was encountered: {:x}", status);
        }
        // defer aborts until this interaction is done
        self.csr.wo(
            utra::mailbox::EV_ENABLE,
            self.csr.r(utra::mailbox::EV_ENABLE) & !self.csr.ms(utra::mailbox::EV_ENABLE_ABORT_INIT, 1),
        );
        // interact with the FIFO
        // note: this only works because rx_words is the LSB of the register. We don't have to shift the MS'd
        // value.
        let rx_words = status & self.csr.ms(utra::mailbox::STATUS_RX_WORDS, !0);
        let rx_words_checked = if rx_words as usize > ret.len() {
            log::warn!("rx_words {} is more than ret.len() {}", rx_words, ret.len());
            drain = true;
            ret.len()
        } else {
            rx_words as usize
        };
        for r in ret[0..rx_words_checked].iter_mut() {
            *r = self.csr.rf(utra::mailbox::RDATA_RDATA);
        }
        // throw away any excess words to avoid breaking the protocol
        for _ in 0..(if drain { 1 } else { 0 } + rx_words as usize - rx_words_checked) {
            let _ = self.csr.rf(utra::mailbox::RDATA_RDATA);
        }
        // re-enable aborts
        self.csr.wo(
            utra::mailbox::EV_ENABLE,
            self.csr.r(utra::mailbox::EV_ENABLE) | self.csr.ms(utra::mailbox::EV_ENABLE_ABORT_INIT, 1),
        );
        rx_words_checked
    }

    pub fn abort(&mut self) {
        log::warn!("abort initiated");
        self.csr.wfo(utra::mailbox::CONTROL_ABORT, 1);
    }
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
    RunTest,
    Incoming,
    AbortInit,
    AbortDone,
    ProtocolError,
    InvalidCall,
    Quit,
}

fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let mbox = unsafe { &mut *(arg as *mut Mailbox) };

    let pending = mbox.csr.r(utra::mailbox::EV_PENDING);
    mbox.csr.wo(utra::mailbox::EV_PENDING, pending);

    if pending & mbox.csr.ms(utra::mailbox::EV_PENDING_ERROR, 1) != 0 {
        let status = mbox.csr.r(utra::mb_client::STATUS); // this clears the error
        xous::try_send_message(
            mbox.cid,
            xous::Message::new_scalar(
                Opcode::ProtocolError.to_usize().unwrap(),
                pending as usize,
                status as usize,
                0,
                0,
            ),
        )
        .ok();
    }
    if pending & mbox.csr.ms(utra::mailbox::EV_PENDING_ABORT_INIT, 1) != 0 {
        mbox.abort_pending = true;
        xous::try_send_message(
            mbox.cid,
            xous::Message::new_scalar(Opcode::AbortInit.to_usize().unwrap(), pending as usize, 0, 0, 0),
        )
        .ok();
    }
    if pending & mbox.csr.ms(utra::mailbox::EV_PENDING_ABORT_DONE, 1) != 0 {
        xous::try_send_message(
            mbox.cid,
            xous::Message::new_scalar(Opcode::AbortDone.to_usize().unwrap(), pending as usize, 0, 0, 0),
        )
        .ok();
    }
    if pending & mbox.csr.ms(utra::mailbox::EV_PENDING_AVAILABLE, 1) != 0 {
        xous::try_send_message(
            mbox.cid,
            xous::Message::new_scalar(Opcode::Incoming.to_usize().unwrap(), pending as usize, 0, 0, 0),
        )
        .ok();
    }
}

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let xns = xous_api_names::XousNames::new().unwrap();
    let mbox_sid = xns.register_name("_mbox_", None).expect("can't register server");
    let mbox_cid = xous::connect(mbox_sid).unwrap();
    log::info!("mbox SID: {:x?}", mbox_sid);

    let mbox_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::mailbox::HW_MAILBOX_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core User CSR range");
    let mbox = CSR::new(mbox_csr.as_mut_ptr() as *mut u32);

    let mut mailbox = Mailbox { csr: mbox, cid: mbox_cid, abort_pending: false };
    #[cfg(not(feature = "ext"))]
    mailbox.csr.wfo(utra::mailbox::LOOPBACK_LOOPBACK, 1);
    xous::claim_interrupt(
        utra::mailbox::MAILBOX_IRQ,
        handle_irq,
        (&mut mailbox) as *mut Mailbox as *mut usize,
    )
    .expect("couldn't claim irq");
    // enable the interrupt
    mailbox.csr.wo(utra::mailbox::EV_ENABLE, !0); // enable everything

    #[cfg(feature = "message-test")]
    {
        #[cfg(feature = "hwsim")]
        let c_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::main::HW_MAIN_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Core Control CSR range");
        #[cfg(feature = "hwsim")]
        let mut core_csr = CSR::new(c_csr.as_mut_ptr() as *mut u32);

        #[cfg(feature = "aestests")]
        {
            use aes::cipher::generic_array::GenericArray;
            use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
            log::info!("AES");
            const TEST_MAX_LEN: usize = 4096;
            let mut dataset_op = [0u8; TEST_MAX_LEN];
            for (i, d) in dataset_op.iter_mut().enumerate() {
                *d = i as u8;
            }
            log::info!("key");
            let key_array: [u8; 32] = [0; 32];
            let key = GenericArray::from_slice(&key_array);
            let cipher_hw = aes::Aes256::new(&key);
            log::info!("round");
            for iter in 0..2 {
                core_csr.wfo(utra::main::REPORT_REPORT, 0xae50_0000 + iter);
                for (i, mut chunk) in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE).enumerate() {
                    let mut block = GenericArray::clone_from_slice(&mut chunk);
                    core_csr.wfo(utra::main::REPORT_REPORT, 0xae5e_0000 + iter + ((i as u32) << 16));
                    cipher_hw.encrypt_block(&mut block);
                    core_csr.wfo(utra::main::REPORT_REPORT, 0xae5e_1000 + iter + ((i as u32) << 16));
                    for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                        *dst = src;
                    }
                }
                for (i, mut chunk) in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE).enumerate() {
                    let mut block = GenericArray::clone_from_slice(&mut chunk);
                    core_csr.wfo(utra::main::REPORT_REPORT, 0xae5d_0000 + iter + ((i as u32) << 16));
                    cipher_hw.decrypt_block(&mut block);
                    core_csr.wfo(utra::main::REPORT_REPORT, 0xae5d_1000 + iter + ((i as u32) << 16));
                    for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                        *dst = src;
                    }
                }
                if iter == 0 {
                    log::info!("0 done");
                }
            }
            log::info!("done");
        }

        let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
        let mut total = 0;
        let mut iter = 0;
        log::info!("running message passing test");
        loop {
            // this conjures a scalar message
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x1111_0000 + iter);
            let now = tt.elapsed_ms();
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x2222_0000 + iter);
            total += now;

            if iter >= 8 && iter < 12 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D001);
                tt.sleep_ms(1).ok();
            } else if iter >= 12 && iter < 13 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D002);
                tt.sleep_ms(2).ok();
            } else if iter >= 13 && iter < 14 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D002);
                tt.sleep_ms(3).ok();
            } else if iter >= 14 {
                break;
            }

            // something lame to just conjure a memory message
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x3333_0000 + iter);
            let version = tt.get_version();
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x4444_0000 + iter);
            total += version.len() as u64;
            iter += 1;
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, now as u32);
            log::info!("message passing test progress: {}ms", tt.elapsed_ms());
        }
        log::info!("total: {}", total); // consume the total value so it's not optimized out
    }
    // tests to run:
    // all tests take the form of loopback, data transmit -> data receive ^ 0xaaaa_0000
    //
    // 1. single packet test
    // 2. 16-word test
    // 3. single packet test (again)
    // 4. 1024-word test
    // 5. single packet test (again)
    // 6. write 8 words, then abort
    // 7. 2-word test
    // 8. write 4 words, transmit. receiver should abort
    // 9. 3-word test

    log::info!("starting mbox test");
    let mut msg_opt = None;
    let mut return_type = 0;
    xous::send_message(
        mailbox.cid,
        xous::Message::new_scalar(Opcode::RunTest.to_usize().unwrap(), 1, 0, 0, 0),
    )
    .ok();

    let mut test_array = [0u32; 1024];
    let mut ret_array = [0u32; 1024];
    let mut generator: u32 = 0x1317_0000;
    let mut abort_init_seen = false;
    let mut abort_done_seen = false;
    let mut expect_error = false; // when testing overflows explicitly
    loop {
        xous::reply_and_receive_next_legacy(mbox_sid, &mut msg_opt, &mut return_type).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        match num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::InvalidCall) {
            Opcode::RunTest => {
                if let Some(scalar) = msg.body.scalar_message() {
                    log::info!("test case {}", scalar.arg1);
                    match scalar.arg1 {
                        1 => {
                            // format of test is:
                            //   word 0: msb = # of words sent; lsb is test sequence number
                            //   word 1: "generator" value
                            test_array[0] = 0x1_0001;
                            unsafe { mailbox.send_unguarded(&test_array[0..1]) };
                        }
                        2 => {
                            test_array[0] = 0x10_0002; // 16 words sent, test sequence 2
                            for t in test_array[1..16].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            unsafe { mailbox.send_unguarded(&test_array[0..16]) };
                        }
                        3 => {
                            test_array[0] = 0x1_0003; // 1 word sent, test sequence 3
                            unsafe { mailbox.send_unguarded(&test_array[0..1]) };
                        }
                        4 => {
                            test_array[0] = 0x0400_0004; // 1024 words sent, test sequence 4
                            for t in test_array[1..1024].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            expect_error = true;
                            unsafe { mailbox.send_unguarded(&test_array[0..1024]) };
                        }
                        5 => {
                            expect_error = false;
                            test_array[0] = 0x1_0005; // 1 word sent, test sequence 5
                            unsafe { mailbox.send_unguarded(&test_array[0..1]) };
                        }
                        6 => {
                            test_array[0] = 0x8_0006; // 8 words sent, test sequence 6
                            for t in test_array[1..8].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            unsafe { mailbox.send_unguarded(&test_array[0..8]) };
                            mailbox.abort();
                        }
                        7 => {
                            if !abort_done_seen {
                                log::error!("We did not see an abort ack");
                                break;
                            }
                            abort_done_seen = false;
                            test_array[0] = 0x0002_0007; // 2 words sent, test sequence 7
                            for t in test_array[1..2].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            unsafe { mailbox.send_unguarded(&test_array[0..2]) };
                        }
                        8 => {
                            abort_init_seen = false;
                            test_array[0] = 0x4_0008; // 4 words sent, test sequence 8
                            for t in test_array[1..4].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            unsafe { mailbox.send_unguarded(&test_array[0..4]) };
                        }
                        9 => {
                            if !abort_init_seen {
                                log::error!("We did not see the other side initiate an abort");
                            }
                            abort_init_seen = false;

                            test_array[0] = 0x3_0009; // 3 words sent, test sequence 9
                            for t in test_array[1..3].iter_mut() {
                                *t = generator;
                                generator += 1;
                            }
                            unsafe { mailbox.send_unguarded(&test_array[0..3]) };
                        }
                        10 => {
                            for (i, d) in test_array.iter_mut().enumerate() {
                                *d = i as u32;
                            }
                            test_array[0] = 0x0400_000A; // 1024 words sent, test sequence 10
                            unsafe { mailbox.send_unguarded(&test_array) };
                        }
                        11 => {
                            log::info!("Last test done");
                            break;
                        }
                        _ => {
                            log::error!("Incorrect test sequence received: {}, test terminated", scalar.arg1);
                        }
                    }
                } else {
                    log::error!("Wrong message type for RunTest");
                }
            }
            Opcode::Incoming => {
                if mailbox.abort_pending {
                    log::info!("Got abort in between rx IRQ and rx handler");
                    // ignore the packet, let the abort handler run
                    continue;
                }
                if let Some(_scalar) = msg.body.scalar_message() {
                    let count = mailbox.get(&mut ret_array);
                    if check_results(&test_array, &ret_array, count) {
                        let last_seq_no = test_array[0] & 0xffff;
                        log::info!("Test {} passed", last_seq_no);
                        xous::send_message(
                            mailbox.cid,
                            xous::Message::new_scalar(
                                Opcode::RunTest.to_usize().unwrap(),
                                last_seq_no as usize + 1,
                                0,
                                0,
                                0,
                            ),
                        )
                        .ok();
                    } else {
                        log::error!("Aborting test, errors encountered");
                        break;
                    }
                } else {
                    log::error!("Wrong message type for RunTest");
                }
            }
            Opcode::AbortInit => {
                abort_init_seen = true;
                mailbox.abort_pending = false;
                log::info!("test peer initiated abort!");
                // acknowledge the abort
                mailbox.csr.wfo(utra::mailbox::CONTROL_ABORT, 1);
                // initiate the next test in the sequence
                let last_seq_no = test_array[0] & 0xffff;
                xous::send_message(
                    mailbox.cid,
                    xous::Message::new_scalar(
                        Opcode::RunTest.to_usize().unwrap(),
                        last_seq_no as usize + 1,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
            }
            Opcode::AbortDone => {
                abort_done_seen = true;
                mailbox.abort_pending = false;
                log::info!("abort protocol done");
                // initiate the next test in the sequence
                let last_seq_no = test_array[0] & 0xffff;
                xous::send_message(
                    mailbox.cid,
                    xous::Message::new_scalar(
                        Opcode::RunTest.to_usize().unwrap(),
                        last_seq_no as usize + 1,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
            }
            Opcode::ProtocolError => {
                if let Some(scalar) = msg.body.scalar_message() {
                    if !expect_error {
                        log::error!(
                            "Protocol error received: {:x}, {:x}, aborting test",
                            scalar.arg1,
                            scalar.arg2
                        );
                        break;
                    } else {
                        log::info!("Expected protocol error received: {:x}, {:x}", scalar.arg1, scalar.arg2);
                    }
                } else {
                    log::error!("Wrong message type for ProtocolError; aborting test");
                    break;
                }
            }
            Opcode::InvalidCall => {
                log::error!("Invalid opcode: {:?}", msg);
            }
            Opcode::Quit => {
                break;
            }
        }
    }
}

fn check_results(test_array: &[u32], ret_array: &[u32], count: usize) -> bool {
    let test_len = (test_array[0] >> 16) as usize;
    if test_len > test_array.len() || test_len > ret_array.len() || test_len != count {
        log::error!(
            "Test length is incorrect: expected {:x}, got {:x} [{:x}]",
            test_len,
            count,
            test_array[0]
        );
        return false;
    }
    let mut errcnt = 0;
    for (index, (&tx, &rx)) in test_array[0..test_len].iter().zip(&ret_array[0..test_len]).enumerate() {
        if rx != tx ^ 0xAAAA_0000 {
            if errcnt < 16 {
                // limit log spew
                log::error!("Test failure at {}: {:x}->{:x}", index, tx ^ 0xAAAA_0000, rx);
            }
            errcnt += 1;
        }
    }
    if errcnt == 0 {
        log::info!("Test passed with length {}", test_len);
        true
    } else {
        log::error!("Test failed with length {}", test_len);
        false
    }
}
