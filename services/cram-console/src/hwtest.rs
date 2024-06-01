use utralib::generated::*;

pub fn hwtest() {
    #[cfg(feature = "hwsim")]
    let csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::main::HW_MAIN_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core Control CSR range");
    #[cfg(feature = "hwsim")]
    let mut core_csr = Some(CSR::new(csr.as_mut_ptr() as *mut u32));

    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_0000);

    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0xa51d_0000);
    let coreuser_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::coreuser::HW_COREUSER_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core User CSR range");
    let mut coreuser = CSR::new(coreuser_csr.as_mut_ptr() as *mut u32);
    // first, clear the ASID table to 0
    for asid in 0..512 {
        coreuser.wo(
            utra::coreuser::SET_ASID,
            coreuser.ms(utra::coreuser::SET_ASID_ASID, asid)
                | coreuser.ms(utra::coreuser::SET_ASID_TRUSTED, 0),
        );
    }
    // set my PID to trusted
    coreuser.wo(
        utra::coreuser::SET_ASID,
        coreuser.ms(utra::coreuser::SET_ASID_ASID, xous::process::id() as u32)
            | coreuser.ms(utra::coreuser::SET_ASID_TRUSTED, 1),
    );
    // set the required `mpp` state to user code (mpp == 0)
    coreuser.wfo(utra::coreuser::SET_PRIVILEGE_MPP, 0);
    // turn on the coreuser computation
    coreuser.wo(
        utra::coreuser::CONTROL,
        coreuser.ms(utra::coreuser::CONTROL_ASID, 1)
            | coreuser.ms(utra::coreuser::CONTROL_ENABLE, 1)
            | coreuser.ms(utra::coreuser::CONTROL_PRIVILEGE, 1),
    );
    // turn off coreuser control updates
    coreuser.wo(utra::coreuser::PROTECT, 1);
    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0xa51d_600d);

    log::info!("my PID is {}", xous::process::id());

    #[cfg(feature = "pio-test")]
    {
        log::info!("running PIO tests");
        xous_pio::pio_tests::pio_tests();
        log::info!("resuming console tests");
    }

    #[cfg(feature = "pl230-test")]
    {
        log::info!("running PL230 tests");
        xous_pl230::pl230_tests::pl230_tests();
        log::info!("resuming console tests");
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
    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0x6969_6969);
    println!("Elapsed: {}", total);
    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_c0de);

    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::SUCCESS_SUCCESS, 1);
    tt.sleep_ms(4).ok();
    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::DONE_DONE, 1); // this should stop the simulation
    log::info!("message passing test done at {}ms!", tt.elapsed_ms());
}
