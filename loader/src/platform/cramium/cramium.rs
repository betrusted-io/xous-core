#[cfg(not(feature = "verilator-only"))]
use cramium_api::{
    udma::{PeriphId, UdmaGlobalConfig},
    *,
};
#[cfg(not(feature = "verilator-only"))]
use cramium_hal::iox::Iox;
#[cfg(feature = "cramium-soc")]
use cramium_hal::udma;
#[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
use cramium_hal::{axp2101::Axp2101, udma::GlobalConfig};
use utralib::generated::*;
use utralib::utra::sysctrl;

#[cfg(feature = "qr")]
use crate::platform::cramium::{homography, qr};

#[global_allocator]
#[cfg(not(feature = "usb"))]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

// Notes about the reset vector location
// This can be set using fuses in the IFR (also called 'info') region
// The offset is an 8-bit value, which is shifted into a final location
// according to the following formula:
//
// let short_offset: u8 = OFFSET;
// let phys_offset: u32 = 0x6000_0000 + short_offset << 14;
//
// The RV32-IV IFR fuse location is at row 6, byte 8.
// Each row is 256 bits wide.
// This puts the byte-address hex offset at (6 * 256 + 8 * 8) / 8 = 0xC8
// within the IFR region. Total IFR region size is 0x200.

/*
  Thoughts on where to put the updating routine.

  As a Xous runtime program:
    Pros:
      - Full security of MMU
      - Updater primitives are available as Xous primitives
    Cons:
      - Less RAM available to stage data
      - ReRAM is XIP; makes kernel update tricky
  As a loader program:
    Pros:
      - Higher performance (no OS overhead)
      - Faster dev time
      - Can stage full images in PSRAM before committing
      - Full overwrite of ReRAM OS possible
    Cons:
      - Larger loader image
      - No MMU security - bugs are more brittle/scary
      - Loader becomes a primary attack surface due to its size and complexity
      - Less code re-use with main code base
      - Loader update needs a special path to hand-off an image to Xous to avoid XIP conflict

   I think the winning argument is that we could stage the full image in PSRAM before
   committing to either ReRAM or SPI RAM. This allows us to do full signature checking prior
   to committing any objects.

   We could structure this so that when going into update mode, the secret key lifecycle
   bits are pushed forward, so we only have derived keys available. This would make any
   break into the loader updater less able to get at any root keys?
*/

pub const SYSTEM_CLOCK_FREQUENCY: u32 = 800_000_000;

// Define the .data region - bootstrap baremetal using these hard-coded parameters.
pub const DATA_ORIGIN: usize = 0x61000000;
pub const DATA_SIZE_BYTES: usize = 0x5000;
pub const DATA_INIT: [(usize, u32); 1] = [(0x0, 0x2)];

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;

// location of kernel, as offset from the base of ReRAM. This needs to match up with what is in link.x.
// exclusive of the signature block offset
pub const KERNEL_OFFSET: usize = 0x6_0000;

#[allow(dead_code)]
pub fn delay(quantum: usize) {
    use utralib::{CSR, utra};
    // abuse the d11ctime timer to create some time-out like thing
    let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
    d11c.wfo(utra::d11ctime::CONTROL_COUNT, 333_333); // 1.0ms per interval
    let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    for _ in 0..quantum {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
        polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    }
    // we have to split this because we don't know where we caught the previous interval
    if quantum == 1 {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
    }
}

#[cfg(all(feature = "cramium-soc", not(feature = "verilator-only")))]
pub fn early_init() -> u32 {
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    let ao_sysctrl = utra::ao_sysctrl::HW_AO_SYSCTRL_BASE as *mut u32;
    unsafe {
        // this turns off the VDD85D (doesn't work)
        // TODO: debug switching regulator power-on
        // ao_sysctrl.add(utra::ao_sysctrl::SFR_PMUCSR.offset()).write_volatile(0x6c);

        // this sets VDD85D to 0.90V
        ao_sysctrl.add(utra::ao_sysctrl::SFR_PMUTRM0CSR.offset()).write_volatile(0x0842_1FF1); // 0x0842_1080 default
        daric_cgu.add(utra::sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x57);
    }

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    unsafe {
        let data_ptr = DATA_ORIGIN as *mut u32;
        for i in 0..DATA_SIZE_BYTES / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for (offset, data) in DATA_INIT {
            data_ptr.add(offset).write_volatile(data);
        }
    }

    unsafe {
        // this block is mandatory in all cases to get clocks set into some consistent, expected mode
        {
            // conservative dividers
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x3f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x1f3f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0f1f);
            // ungate all clocks
            daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xFF);
            // commit clocks
            daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        // enable DUART
        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        // based on ringosc trimming as measured by oscope. this will get precise after we set the PLL.
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(34);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }
    // sram0 requires 1 wait state for writes
    let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM0, 0x8);
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM1, 0x8);

    // #[cfg(feature = "v0p9")] // always targeting 0.9v timings
    {
        /*
        logic [15:0] trm_ram32kx72      ; assign trm_ram32kx72      = trmdat[0 ]; localparam t_trm IV_trm_ram32kx72      = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_ram8kx72       ; assign trm_ram8kx72       = trmdat[1 ]; localparam t_trm IV_trm_ram8kx72       = IV_sram_sp_hde_inst_sram1;
        logic [15:0] trm_rf1kx72        ; assign trm_rf1kx72        = trmdat[2 ]; localparam t_trm IV_trm_rf1kx72        = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf256x27       ; assign trm_rf256x27       = trmdat[3 ]; localparam t_trm IV_trm_rf256x27       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf512x39       ; assign trm_rf512x39       = trmdat[4 ]; localparam t_trm IV_trm_rf512x39       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf128x31       ; assign trm_rf128x31       = trmdat[5 ]; localparam t_trm IV_trm_rf128x31       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_dtcm8kx36      ; assign trm_dtcm8kx36      = trmdat[6 ]; localparam t_trm IV_trm_dtcm8kx36      = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_itcm32kx18     ; assign trm_itcm32kx18     = trmdat[7 ]; localparam t_trm IV_trm_itcm32kx18     = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_ifram32kx36    ; assign trm_ifram32kx36    = trmdat[8 ]; localparam t_trm IV_trm_ifram32kx36    = IV_sram_sp_uhde_inst;
        logic [15:0] trm_sce_sceram_10k ; assign trm_sce_sceram_10k = trmdat[9 ]; localparam t_trm IV_trm_sce_sceram_10k = IV_sram_sp_hde_inst;
        logic [15:0] trm_sce_hashram_3k ; assign trm_sce_hashram_3k = trmdat[10]; localparam t_trm IV_trm_sce_hashram_3k = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aesram_1k  ; assign trm_sce_aesram_1k  = trmdat[11]; localparam t_trm IV_trm_sce_aesram_1k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_pkeram_4k  ; assign trm_sce_pkeram_4k  = trmdat[12]; localparam t_trm IV_trm_sce_pkeram_4k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aluram_3k  ; assign trm_sce_aluram_3k  = trmdat[13]; localparam t_trm IV_trm_sce_aluram_3k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_mimmdpram  ; assign trm_sce_mimmdpram  = trmdat[14]; localparam t_trm IV_trm_sce_mimmdpram  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rdram1kx32     ; assign trm_rdram1kx32     = trmdat[15]; localparam t_trm IV_trm_rdram1kx32     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram512x64    ; assign trm_rdram512x64    = trmdat[16]; localparam t_trm IV_trm_rdram512x64    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram128x22    ; assign trm_rdram128x22    = trmdat[17]; localparam t_trm IV_trm_rdram128x22    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram32x16     ; assign trm_rdram32x16     = trmdat[18]; localparam t_trm IV_trm_rdram32x16     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_bioram1kx32    ; assign trm_bioram1kx32    = trmdat[19]; localparam t_trm IV_trm_bioram1kx32    = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_tx_fifo128x32  ; assign trm_tx_fifo128x32  = trmdat[20]; localparam t_trm IV_trm_tx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rx_fifo128x32  ; assign trm_rx_fifo128x32  = trmdat[21]; localparam t_trm IV_trm_rx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_fifo32x19      ; assign trm_fifo32x19      = trmdat[22]; localparam t_trm IV_trm_fifo32x19      = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_share   ; assign trm_udcmem_share   = trmdat[23]; localparam t_trm IV_trm_udcmem_share   = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_odb     ; assign trm_udcmem_odb     = trmdat[24]; localparam t_trm IV_trm_udcmem_odb     = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_256x64  ; assign trm_udcmem_256x64  = trmdat[25]; localparam t_trm IV_trm_udcmem_256x64  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_acram2kx64     ; assign trm_acram2kx64     = trmdat[26]; localparam t_trm IV_trm_acram2kx64     = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_aoram1kx36     ; assign trm_aoram1kx36     = trmdat[27]; localparam t_trm IV_trm_aoram1kx36     = IV_sram_sp_hde_inst;

             */
        crate::println!("setting 0.9v sramtrm");
        let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
        sramtrm.wo(utra::coresub_sramtrm::SFR_CACHE, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_ITCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_DTCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_VEXRAM, 0x1);

        let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
        // bio 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (19 << 16) | 0b011_000_01_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // vex 0.9v settings
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((15 + i) << 16) | 0b001_010_00_0_0_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }

        // sram 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (0 << 16) | 0b011_000_01_0_1_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (1 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        crate::println!("setting other 0.9v trims");

        // tcm 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (6 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (7 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // ifram 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (8 << 16) | 0b010_000_00_0_1_000_1_01);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // sce 0.9V
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (9 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((10 + i) << 16) | 0b011_000_01_0_1_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (14 << 16) | 0b001_010_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
    }

    // unsafe, direct-writes to address offsets are used here instead of the UTRA abstraction
    // because there are some quirks in the early boot path that make the system more stable
    // if all register accesses are in-lined.
    #[cfg(feature = "boot-delay")]
    unsafe {
        // this block should immediately follow the CGU setup
        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        // ~2 second delay for debugger to attach
        let msg = b"boot\n\r";
        for j in 0..1_000 {
            // variable count of .'s to create a sense of motion on the console
            for _ in 0..j & 0x7 {
                while duart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
                duart.add(utra::duart::SFR_TXD.offset()).write_volatile('.' as char as u32);
            }
            for &b in msg {
                while duart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
                duart.add(utra::duart::SFR_TXD.offset()).write_volatile(b as char as u32);
            }
        }
    }

    // Now setup the clocks for real
    // Safety: this can only be called in the early_init boot context
    #[cfg(not(feature = "simulation-only"))]
    let perclk = unsafe { init_clock_asic(SYSTEM_CLOCK_FREQUENCY) };
    #[cfg(feature = "simulation-only")]
    let perclk = 100_000_000;
    crate::println!("Perclk is {} Hz", perclk);

    // Configure the UDMA UART. This UART's settings will be used as the initial console UART.
    // This is configured in the loader so that the log crate does not have a dependency
    // on the cramium-hal crate to be functional.

    // Set up the IO mux to map UART_A0:
    //  UART_RX_A[0] = PA3   app
    //  UART_TX_A[0] = PA4   app
    //  UART_RX_B[2] = PB13  console
    //  UART_RX_B[2] = PB14  console
    #[allow(unused_mut)] // some configs require mut
    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    iox.set_alternate_function(IoxPort::PB, 13, IoxFunction::AF1);
    iox.set_alternate_function(IoxPort::PB, 14, IoxFunction::AF1);
    // rx as input, with pull-up
    iox.set_gpio_dir(IoxPort::PB, 13, IoxDir::Input);
    iox.set_gpio_pullup(IoxPort::PB, 13, IoxEnable::Enable);
    // tx as output
    iox.set_gpio_dir(IoxPort::PB, 14, IoxDir::Output);
    // Set up the UDMA_UART block to the correct baud rate and enable status
    #[allow(unused_mut)] // some configs require mut
    let mut udma_global = GlobalConfig::new();
    udma_global.clock_on(PeriphId::Uart2);
    udma_global.map_event(
        PeriphId::Uart2,
        PeriphEventType::Uart(EventUartOffset::Rx),
        EventChannel::Channel0,
    );
    udma_global.map_event(
        PeriphId::Uart2,
        PeriphEventType::Uart(EventUartOffset::Tx),
        EventChannel::Channel1,
    );

    let baudrate: u32 = 115200;
    let freq: u32 = perclk / 2;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = loader::UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    crate::println!("Baud freq is {} Hz, baudrate is {}", freq, baudrate);
    udma_uart.set_baud(baudrate, freq);

    // these tests aren't safe, but we have to run them nonetheless
    #[cfg(feature = "clock-tests")]
    unsafe {
        clock_tests(&mut udma_uart);
    }

    // Setup some global control registers that will allow the TRNG to operate once the kernel is
    // booted. This is done so the kernel doesn't have to exclusively have rights to the SCE global
    // registers just for this purpose.
    let mut glbl_csr = CSR::new(utralib::utra::sce_glbsfr::HW_SCE_GLBSFR_BASE as *mut u32);
    glbl_csr.wo(utra::sce_glbsfr::SFR_SUBEN, 0xff);
    glbl_csr.wo(utra::sce_glbsfr::SFR_FFEN, 0x30);
    glbl_csr.wo(utra::sce_glbsfr::SFR_FFCLR, 0xff05);

    // configure LDO voltages that aren't correct by default.
    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    let i2c_channel = cramium_hal::board::setup_i2c_pins(&iox);
    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    udma_global.clock(PeriphId::from(i2c_channel), true);
    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    let i2c_ifram = unsafe {
        cramium_hal::ifram::IframRange::from_raw_parts(
            cramium_hal::board::I2C_IFRAM_ADDR,
            cramium_hal::board::I2C_IFRAM_ADDR,
            4096,
        )
    };

    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    let mut i2c = unsafe {
        cramium_hal::udma::I2c::new_with_ifram(i2c_channel, 400_000, perclk, i2c_ifram, &udma_global)
    };
    // setup PMIC
    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    {
        let mut pmic: Option<Axp2101> = None;

        for _ in 0..3 {
            match cramium_hal::axp2101::Axp2101::new(&mut i2c) {
                Ok(p) => {
                    pmic = Some(p);
                    break;
                }
                Err(e) => {
                    crate::println!("Error initializing pmic: {:?}, retrying", e);

                    // we have to reboot it appears if the I2C is unstable - a "soft recovery"
                    // just leads to CPU lock-up on exit from the init routine? what is going on??
                    // maybe some IFRAM instability? Maybe the I2C unit is locking up?
                    let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
                    rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
                    rcurst.wo(utra::sysctrl::SFR_RCURST1, 0x55AA);

                    delay(500);
                }
            };
        }
        if let Some(mut pmic) = pmic {
            // disable CCM, force PWM on for DCDC2 - required for rail stability
            pmic.set_pwm_mode(&mut i2c, cramium_hal::axp2101::WhichDcDc::Dcdc2, true).unwrap();
            pmic.set_dcdc(&mut i2c, Some((0.9, false)), cramium_hal::axp2101::WhichDcDc::Dcdc2).unwrap();

            // turns on the camera DCDC. TODO: make this turn on only when camera is active.
            pmic.set_ldo(&mut i2c, Some(2.85), cramium_hal::axp2101::WhichLdo::Bldo2).unwrap();
            pmic.set_dcdc(&mut i2c, Some((1.8, false)), cramium_hal::axp2101::WhichDcDc::Dcdc5).unwrap();

            // TODO: cleanup these legacy comments
            /*
            pmic.set_ldo(&mut i2c, Some(2.7), cramium_hal::axp2101::WhichLdo::Aldo2).unwrap();
            pmic.set_dcdc(&mut i2c, Some((1.8, false)), cramium_hal::axp2101::WhichDcDc::Dcdc4).unwrap();
            pmic.set_dcdc(&mut i2c, Some((1.8, false)), cramium_hal::axp2101::WhichDcDc::Dcdc5).unwrap();

            // apply proposed baosec settings
            pmic.debug(&mut i2c);
            */

            // This debug print creates a lot of extra code...
            // crate::println!("AXP2101 configure: {:?}", pmic);

            // test battery power off
            /*
            crate::println!("poweroff");
            pmic.set_ldo(&mut i2c, None, cramium_hal::axp2101::WhichLdo::Aldo3).unwrap();
            crate::println!("poweroff done");
            */

            // try to get the BATTFET disengaged on AXP2101
            // the lowest current we can get is 3mA...
            /*
            let mut buf = [0u8, 0u8];
            crate::println!("debug");
            i2c.i2c_read(0x34, 0u8, &mut buf, false).unwrap();
            crate::println!("0|1 bef: {:x?}", buf);

            buf[0] = 0;
            buf[1] = 0;
            // force batfet off
            i2c.i2c_write(0x34, 0x12u8, &buf[..1]).unwrap();
            crate::println!("delay");
            i2c.i2c_read(0x34, 0u8, &mut buf, false).unwrap();
            crate::println!("0|1 aft: {:x?}", buf);

            crate::println!("enable power off");
            buf[0] = 0x2;
            i2c.i2c_write(0x34, 0x22u8, &buf[..1]).unwrap();

            crate::println!("poweroff");
            buf[0] = 0x1;
            i2c.i2c_write(0x34, 0x10u8, &buf[..1]).unwrap();
            crate::println!("poweroff done");
            */

            // Make this true to have the system shut down by disconnecting its own battery while on battery
            // power Note this does nothing if you have USB power plugged in.
            if false {
                crate::println!("shutting down...");
                pmic.set_ldo(&mut i2c, Some(0.9), cramium_hal::axp2101::WhichLdo::Aldo3).ok();
                crate::println!("system should be off");
            }
        } else {
            crate::println!("Couldn't init AXP2101, rebooting");
            let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
            rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
            rcurst.wo(utra::sysctrl::SFR_RCURST1, 0x55AA);
            panic!("System should have reset");
        }
    }

    // TODO: move this into the camera routine. For now, just free-run the clock; burns a lot of power.
    // Also, the clock is at 12.5MHz, we really want this at 25MHz but maybe it's just not possible?
    /*
    crate::println!("Setup PWM");
    {
        use cramium_api::*;
        use cramium_hal::iox::Iox;
        let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
        iox.setup_pin(IoxPort::PF, 9, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);
        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
        for _ in 0..8 {
            iox.set_gpio_pin(IoxPort::PA, 0, IoxValue::Low);
            iox.set_gpio_pin(IoxPort::PA, 0, IoxValue::High);
        }

        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::AF3),
            None,
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
        let mut timer = CSR::new(utra::pwm::HW_PWM_BASE as *mut u32);
        timer.wo(utra::pwm::REG_CH_EN, 1);
        timer.rmwf(utra::pwm::REG_TIM0_CFG_R_TIMER0_SAW, 0);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_TH, 0);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_MODE, 3);
        let pwm = utra::pwm::HW_PWM_BASE as *mut u32;
        unsafe { pwm.add(2).write_volatile(1 << 16) };
        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
        crate::println!("PWM running on PA0?");
        for i in 0..12 {
            crate::println!("0x{:2x}: 0x{:08x}", i, unsafe { pwm.add(i).read_volatile() })
        }
        crate::println!("0x{:2x}: 0x{:08x}", 65, unsafe { pwm.add(65).read_volatile() });
        crate::println!("");
    }
    */

    #[cfg(feature = "board-bringup")]
    let iox_loop = iox.clone();

    #[cfg(any(feature = "board-baosec", feature = "board-baosor"))]
    {
        // show the boot logo
        use ux_api::minigfx::FrameBuffer;

        let mut udma_global = GlobalConfig::new();
        let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(
            cramium_hal::sh1107::MainThreadToken::new(),
            perclk,
            &mut iox,
            &mut udma_global,
        );
        sh1107.init();
        crate::platform::cramium::bootlogo::show_logo(&mut sh1107);
        sh1107.draw();
        #[cfg(feature = "sh1107-bringup")]
        loop {
            use core::fmt::Write;

            use ux_api::minigfx::Point;
            for i in 0..96 {
                sh1107.buffer_mut().fill(0);
                let native_buf = unsafe { sh1107.raw_mut() };
                let p = if i < 64 { i } else { i - 64 + 128 };
                native_buf[p / 32] = 1 << (p % 32);
                native_buf[0] |= 1;
                use crate::platform::cramium::gfx;
                let mut usizestr = crate::platform::UsizeToString::new();
                write!(usizestr, "{}:[{}]{}", p, p / 32, p % 32).ok();
                gfx::msg(
                    &mut sh1107,
                    usizestr.as_str(),
                    Point::new(20, 64),
                    cramium_hal::sh1107::Mono::White.into(),
                    cramium_hal::sh1107::Mono::Black.into(),
                );
                sh1107.draw();
                delay(250);
            }
        }
        //------------- test camera ------------
        #[cfg(feature = "cam-test")]
        {
            use cramium_api::iox::IoxValue;
            use cramium_hal::sh1107::Mono;
            use cramium_hal::udma::Udma;
            use ux_api::minigfx::Point;

            use crate::platform::cramium::gfx;
            use crate::platform::cramium::qr;

            let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
            let udma_global = GlobalConfig::new();

            // setup camera pins
            let (cam_pdwn_bnk, cam_pdwn_pin) = cramium_hal::board::setup_camera_pins(&iox);
            // disable camera powerdown
            iox.set_gpio_pin(cam_pdwn_bnk, cam_pdwn_pin, IoxValue::Low);
            udma_global.clock_on(PeriphId::Cam);
            let cam_ifram = unsafe {
                cramium_hal::ifram::IframRange::from_raw_parts(
                    cramium_hal::board::CAM_IFRAM_ADDR,
                    cramium_hal::board::CAM_IFRAM_ADDR,
                    cramium_hal::board::CAM_IFRAM_LEN_PAGES * 4096,
                )
            };
            // this is safe because we turned on the clocks before calling it
            let mut cam = unsafe { cramium_hal::gc2145::Gc2145::new_with_ifram(cam_ifram) };

            cam.delay(100);
            let (pid, mid) = cam.read_id(&mut i2c);
            crate::println!("Camera pid {:x}, did {:x}", pid, mid);
            crate::println!("enter init");
            cam.init(&mut i2c, cramium_api::camera::Resolution::Res320x240);
            cam.delay(1);

            const QR_WIDTH: usize = 256;
            const QR_HEIGHT: usize = 240;

            let (cols, _rows) = cam.resolution();
            let border = (cols - QR_WIDTH) / 2;
            cam.set_slicing((border, 0), (cols - border, QR_HEIGHT));
            crate::println!("320x240 resolution setup with 256x240 slicing");

            let mut csr_tt = CSR::new(utra::ticktimer::HW_TICKTIMER_BASE as *mut u32);
            csr_tt.wfo(utra::ticktimer::CLOCKS_PER_TICK_CLOCKS_PER_TICK, 200_000);
            csr_tt.wfo(utra::ticktimer::CONTROL_RESET, 1);

            let iox_loop = iox.clone();
            let mut frames = 0;
            let mut frame = [0u8; QR_WIDTH * QR_HEIGHT];
            while iox_loop.get_gpio_pin(IoxPort::PB, 9) == IoxValue::High {}
            udma_uart.setup_async_read();
            let mut c = 0u8;
            let mut bw_thresh: u8 = 131;
            loop {
                // toggling this off can improve performance by wasting less time "waiting" for the next
                // frame... however, you will get "frame rolling" if the capture isn't
                // initiated at exactly the right time. things could be improved by making
                // this interrupt-driven. maybe the same effect could also be achieved with
                // frame dropping? not sure. to be researched.
                while iox_loop.get_gpio_pin(IoxPort::PB, 9) == IoxValue::High {}

                cam.capture_async();

                // blit fb to sh1107
                let mut sum: u32 = 0;
                let mut count: u32 = 0;
                for (y, row) in frame.chunks(QR_WIDTH).enumerate() {
                    if y & 1 == 0 {
                        for (x, &pixval) in row.iter().enumerate() {
                            // note: image rotation is done here by swapping x/y coords in the lower equations
                            if x & 1 == 0 {
                                if y < sh1107.dimensions().x as usize * 2
                                    && x < sh1107.dimensions().y as usize * 2
                                        - (gfx::CHAR_HEIGHT as usize + 1) * 2
                                {
                                    let luminance = pixval & 0xff;
                                    count += 1;
                                    sum += luminance as u32;
                                    if luminance > bw_thresh {
                                        // flip on y to adjust for sensor orientation. Lower left is (0, 0)
                                        // on the display.
                                        sh1107.put_pixel(
                                            Point::new(
                                                y as isize / 2,
                                                (sh1107.dimensions().y - 1) - (x as isize / 2),
                                            ),
                                            Mono::White.into(),
                                        );
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                bw_thresh = (sum / count) as u8;

                let mut candidates: [Option<Point>; 64] = [None; 64];
                crate::println!("\n\r------------- SEARCH -----------");
                let finder_width = qr::find_finders(&mut candidates, &frame, bw_thresh, QR_WIDTH) as isize;
                const CROSSHAIR_LEN: isize = 3;
                let mut candidates_found = 0;
                let mut candidate3 = [Point::new(0, 0); 3];
                for candidate in candidates.iter() {
                    if let Some(c) = candidate {
                        if candidates_found < candidate3.len() {
                            candidate3[candidates_found] = *c;
                        }
                        candidates_found += 1;
                        crate::println!("******    candidate: {}, {}    ******", c.x, c.y);
                        // remap image to screen coordinates (it's 2:1)
                        let mut c_screen = *c / 2;
                        // flip coordinates to match the camera data
                        c_screen = Point::new(c_screen.x, sh1107.dimensions().y - 1 - c_screen.y);
                        draw_crosshair(&mut sh1107, c_screen);
                    }
                }
                use crate::platform::cramium::qr::*;
                if candidates_found == 3 {
                    let maybe_qr_corners = QrCorners::from_finders(
                        &candidate3,
                        Point::new(QR_WIDTH as isize, QR_HEIGHT as isize),
                        // add a search margin on the finder width
                        (finder_width
                            + (crate::platform::cramium::qr::FINDER_SEARCH_MARGIN * finder_width)
                                / (1 + 1 + 3 + 1 + 1)) as usize,
                    );
                    // just doing this to avoid nesting another if level out in the test code; make this
                    // better!
                    if maybe_qr_corners.is_none() {
                        continue;
                    }
                    let mut qr_corners = maybe_qr_corners.unwrap();

                    let dims = Point::new(QR_WIDTH as isize, QR_HEIGHT as isize);
                    let mut il = ImageRoi::new(&mut frame, dims, bw_thresh);
                    let (src, dst) =
                        qr_corners.mapping(&mut il, crate::platform::cramium::qr::HOMOGRAPHY_MARGIN);
                    for s in src.iter() {
                        if let Some(p) = s {
                            crate::println!("src {:?}", p);
                            draw_crosshair(&mut sh1107, *p / 2);
                        }
                    }
                    for d in dst.iter() {
                        if let Some(p) = d {
                            crate::println!("dst {:?}", p);
                            draw_crosshair(&mut sh1107, *p / 2);
                        }
                    }

                    let mut src_f: [(f32, f32); 4] = [(0.0, 0.0); 4];
                    let mut dst_f: [(f32, f32); 4] = [(0.0, 0.0); 4];
                    let mut all_found = true;
                    for (s, s_f32) in src.iter().zip(src_f.iter_mut()) {
                        if let Some(p) = s {
                            *s_f32 = p.to_f32();
                        } else {
                            all_found = false;
                        }
                    }
                    for (d, d_f32) in dst.iter().zip(dst_f.iter_mut()) {
                        if let Some(p) = d {
                            *d_f32 = p.to_f32();
                        } else {
                            all_found = false;
                        }
                    }

                    if all_found {
                        use crate::platform::cramium::homography::*;
                        if let Some(h) = find_homography(src_f, dst_f) {
                            if let Some(h_inv) = h.try_inverse() {
                                // crate::println!("{:?}", h_inv);
                                let h_inv_fp = matrix3_to_fixp(h_inv);
                                // crate::println!("{:?}", h_inv_fp);

                                // apply homography to generate a new buffer for processing
                                let mut aligned = [0u8; QR_WIDTH * QR_HEIGHT];
                                // iterate through pixels and apply homography
                                for y in 0..dims.y {
                                    for x in 0..dims.x {
                                        let (x_src, y_src) =
                                            apply_fixp_homography(&h_inv_fp, (x as i32, y as i32));
                                        if (x_src as i32 >= 0)
                                            && ((x_src as i32) < dims.x as i32)
                                            && (y_src as i32 >= 0)
                                            && ((y_src as i32) < dims.y as i32)
                                        {
                                            // println!("{},{} -> {},{}", x_src as i32, y_src as i32, x, y);
                                            aligned[QR_WIDTH * y as usize + x as usize] =
                                                frame[QR_WIDTH * y_src as usize + x_src as usize];
                                        } else {
                                            aligned[QR_WIDTH * y as usize + x as usize] = 255;
                                        }
                                    }
                                }

                                // blit aligned to sh1107
                                for (y, row) in aligned.chunks(QR_WIDTH).enumerate() {
                                    if y & 1 == 0 {
                                        for (x, &pixval) in row.iter().enumerate() {
                                            if x & 1 == 0 {
                                                if x < sh1107.dimensions().x as usize * 2
                                                    && y < sh1107.dimensions().y as usize * 2
                                                        - (gfx::CHAR_HEIGHT as usize + 1) * 2
                                                {
                                                    let luminance = pixval & 0xff;
                                                    if luminance > bw_thresh {
                                                        // flip on y to adjust for sensor orientation. Lower
                                                        // left
                                                        // is (0, 0)
                                                        // on the display.
                                                        sh1107.put_pixel(
                                                            Point::new(
                                                                x as isize / 2,
                                                                (sh1107.dimensions().y - 1)
                                                                    - (y as isize / 2),
                                                            ),
                                                            Mono::White.into(),
                                                        );
                                                    } else {
                                                        sh1107.put_pixel(
                                                            Point::new(
                                                                x as isize / 2,
                                                                (sh1107.dimensions().y - 1)
                                                                    - (y as isize / 2),
                                                            ),
                                                            Mono::Black.into(),
                                                        );
                                                    }
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }

                                gfx::msg(
                                    &mut sh1107,
                                    "Aligning...",
                                    Point::new(0, 0),
                                    Mono::White.into(),
                                    Mono::Black.into(),
                                );

                                if udma_uart.read_async(&mut c) != 0 {
                                    if c == ' ' as u32 as u8 {
                                        crate::println!("Dumping...");

                                        crate::println!("frame {}", frames);
                                        udma_uart.write(
                                            "------------------ divider ------------------\n\r".as_bytes(),
                                        );
                                        let hex_to_ascii = [
                                            '0' as u32 as u8,
                                            '1' as u32 as u8,
                                            '2' as u32 as u8,
                                            '3' as u32 as u8,
                                            '4' as u32 as u8,
                                            '5' as u32 as u8,
                                            '6' as u32 as u8,
                                            '7' as u32 as u8,
                                            '8' as u32 as u8,
                                            '9' as u32 as u8,
                                            'A' as u32 as u8,
                                            'B' as u32 as u8,
                                            'C' as u32 as u8,
                                            'D' as u32 as u8,
                                            'E' as u32 as u8,
                                            'F' as u32 as u8,
                                        ];
                                        for line in frame.chunks(32) {
                                            let mut output = [0u8; 3 * 32 + 3];
                                            for (i, &b) in line.iter().enumerate() {
                                                output[i * 3 + 0] = hex_to_ascii[(b >> 4) as usize];
                                                output[i * 3 + 1] = hex_to_ascii[(b & 0xF) as usize];
                                                output[i * 3 + 2] = ',' as u32 as u8;
                                            }
                                            output[3 * 32] = '\\' as u32 as u8;
                                            output[3 * 32 + 1] = '\n' as u32 as u8;
                                            output[3 * 32 + 2] = '\r' as u32 as u8;
                                            udma_uart.write(&output);
                                        }
                                        // continue with boot
                                        break;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    gfx::msg(
                        &mut sh1107,
                        "Searching...",
                        Point::new(0, 0),
                        Mono::White.into(),
                        Mono::Black.into(),
                    );
                }

                // crate::println!("frame {}", frames);
                sh1107.draw();

                // clear the front buffer
                sh1107.clear();

                // inspect camera values
                if false {
                    crate::println!("RX_SADDR {:x}", cam.csr().r(utra::udma_camera::REG_RX_SADDR));
                    crate::println!("RX_SIZE {:x}", cam.csr().r(utra::udma_camera::REG_RX_SIZE));
                    crate::println!("RX_CFG {:x}", cam.csr().r(utra::udma_camera::REG_RX_CFG));
                    crate::println!("CAM_GLOB {:x}", cam.csr().r(utra::udma_camera::REG_CAM_CFG_GLOB));
                    crate::println!("CAM_LL {:x}", cam.csr().r(utra::udma_camera::REG_CAM_CFG_LL));
                    crate::println!("CAM_UR {:x}", cam.csr().r(utra::udma_camera::REG_CAM_CFG_UR));
                    crate::println!("CAM_CFG_SIZE {:x}", cam.csr().r(utra::udma_camera::REG_CAM_CFG_SIZE));
                    crate::println!(
                        "CAM_CFG_FILTER {:x}",
                        cam.csr().r(utra::udma_camera::REG_CAM_CFG_FILTER)
                    );
                    crate::println!("CAM_SYNC {:x}", cam.csr().r(utra::udma_camera::REG_CAM_VSYNC_POLARITY));
                }

                // wait for the transfer to finish
                cam.capture_await(false);
                let fb: &[u32] = cam.rx_buf();

                // fb is non-cacheable, slow memory. If we stride through it in u16 chunks, we end
                // up fetching each location *twice*, because the native width of the bus is a u32
                // Stride through the slice as a u32, allowing us to make the most out of each slow
                // read from IFRAM, and unpack the values into fast SRAM.
                for (&u32src, u8dest) in fb.iter().zip(frame.chunks_mut(2)) {
                    // note that GC2145 puts Y in top bytes of u16s, OV2640 puts them in the lower bytses of
                    // u16's.
                    u8dest[0] = ((u32src >> 8) & 0xff) as u8;
                    u8dest[1] = ((u32src >> 24) & 0xff) as u8;
                }
                frames += 1;
            }
        }
    }

    // Board bring-up: send characters to confirm the UART is configured & ready to go for the logging crate!
    // The "boot gutter" also has a role to pause the system in "real mode" before VM is mapped in Xous
    // makes things a little bit cleaner for JTAG ops, it seems.
    #[cfg(feature = "board-bringup")]
    {
        use cramium_hal::{iox::IoxValue, minigfx::ColorNative, sh1107::Mono, udma::Udma};
        use ux_api::minigfx::{Line, Point};

        use crate::platform::cramium::gfx;
        //------------- test I2C ------------
        crate::println!("i2c test");
        let mut id = [0u8; 8];
        crate::println!("read USB ID");
        i2c.i2c_read_async(0x47, 0, id.len(), false).expect("couldn't initiate read");
        // this let is necessary to get `id` to go out of scope
        i2c.i2c_await(Some(&mut id), false).unwrap();
        crate::println!("ID result: {:x?}", id);

        let mut ldo = [0u8; 11];
        crate::println!("AXP2101 LDO");
        i2c.i2c_read_async(0x34, 0x90, ldo.len(), false).expect("couldn't initiate read");
        i2c.i2c_await(Some(&mut ldo), false).unwrap();
        crate::println!("LDO result: {:x?}", ldo);

        crate::println!("write1");
        ldo[10] = 0xd;
        i2c.i2c_write_async(0x34, 0x90, &ldo).expect("couldn't initiate write");
        i2c.i2c_await(None, false).unwrap();
        ldo.fill(0);

        crate::println!("AXP2101 LDO - last value should be 0xd");
        i2c.i2c_read_async(0x34, 0x90, ldo.len(), false).expect("couldn't initiate read");
        i2c.i2c_await(Some(&mut ldo), false).unwrap();
        crate::println!("LDO result - last value should be 0xd: {:x?}", ldo);

        crate::println!("write2");
        ldo[10] = 0xe;
        i2c.i2c_write_async(0x34, 0x90, &ldo).expect("couldn't initiate write");
        i2c.i2c_await(None, false).unwrap();
        ldo.fill(0);

        crate::println!("AXP2101 LDO - last value should be 0xe");
        i2c.i2c_read_async(0x34, 0x90, ldo.len(), false).expect("couldn't initiate read");
        i2c.i2c_await(Some(&mut ldo), false).unwrap();
        crate::println!("LDO result - last value should be 0xe: {:x?}", ldo);

        //------------- test USB ---------------
        #[cfg(feature = "usb-test")]
        {
            crate::platform::cramium::usb::init_usb();
            // this does not return if USB is initialized correctly...
            unsafe {
                crate::platform::cramium::usb::test_usb();
            }
        }
    }

    #[cfg(feature = "board-bringup")]
    {
        // ---------------- test TRNG -------------------
        // configure the SCE clocks to enable the TRNG
        let mut sce = CSR::new(HW_SCE_GLBSFR_BASE as *mut u32);
        sce.wo(utra::sce_glbsfr::SFR_SUBEN, 0xFF);
        sce.wo(utra::sce_glbsfr::SFR_FFEN, 0x30);

        // do a quick TRNG test.
        let mut trng = cramium_hal::sce::trng::Trng::new(HW_TRNG_BASE);
        trng.setup_raw_generation(32);
        for _ in 0..12 {
            crate::println!("trng raw: {:x}", trng.get_u32().unwrap_or(0xDEAD_BEEF));
        }
        let trng_csr = CSR::new(HW_TRNG_BASE as *mut u32);
        crate::println!("trng status: {:x}", trng_csr.r(utra::trng::SFR_SR));

        const BANNER: &'static str = "\n\rKeep pressing keys to continue boot...\r\n";
        udma_uart.write(BANNER.as_bytes());

        // Quantum timer stub
        #[cfg(feature = "quantum-timer-test")]
        {
            let mut pio_ss = xous_pio::PioSharedState::new();
            let mut sm_a = pio_ss.alloc_sm().unwrap();

            pio_ss.clear_instruction_memory();
            #[rustfmt::skip]
            let timer_code = pio_proc::pio_asm!(
                "restart:",
                "set x, 6",  // 4 cycles overhead gets us to 10 iterations per pulse
                "waitloop:",
                "mov pins, x",
                "jmp x-- waitloop",
                "irq set 0",
                "jmp restart",
            );
            // iox.set_gpio_dir(cramium_hal::iox::IoxPort::PB, 15, cramium_hal::iox::IoxDir::Output);
            let a_prog = xous_pio::LoadedProg::load(timer_code.program, &mut pio_ss).unwrap();
            sm_a.sm_set_enabled(false);
            a_prog.setup_default_config(&mut sm_a);
            sm_a.config_set_out_pins(16, 8);
            sm_a.config_set_clkdiv(50_000.0f32); // set to 1ms per cycle
            iox.set_pio_bit_from_port_and_pin(cramium_hal::iox::IoxPort::PC, 2).unwrap();
            iox.set_pio_bit_from_port_and_pin(cramium_hal::iox::IoxPort::PC, 1).unwrap();
            let pin = iox.set_pio_bit_from_port_and_pin(cramium_hal::iox::IoxPort::PC, 0).unwrap();
            let pin = 0;
            sm_a.sm_set_pindirs_with_mask(7 << 16, 7 << 16);
            sm_a.sm_set_pins_with_mask(7 << 16, 7 << 16);
            //sm_a.sm_set_pindirs_with_mask(1 << pin as usize, 1 << pin as usize);
            //sm_a.sm_set_pins_with_mask(1 << pin as usize, 1 << pin as usize);
            sm_a.sm_init(a_prog.entry());
            sm_a.sm_irq0_source_enabled(xous_pio::PioIntSource::Sm, true);
            sm_a.sm_set_enabled(true);
            crate::println!("pio setup: pin {}", pin);
            loop {
                let status = sm_a.sm_irq0_status(None);
                crate::println!(
                    "pio irq {}({:x}), {}, {:x}, {:x}/{:x}",
                    status,
                    sm_a.pio.r(utra::rp_pio::SFR_IRQ0_INTS),
                    sm_a.sm_address(),
                    sm_a.pio.r(utra::rp_pio::SFR_DBG_PADOUT),
                    sm_a.pio.r(utra::rp_pio::SFR_DBG_PADOE),
                    iox.csr.r(utra::iox::SFR_PIOSEL),
                );
                if status {
                    sm_a.sm_interrupt_clear(0);
                }
            }
        }
        // space for one character, plus appending CRLF for the return
        let mut rx_buf = [0u8; 3];

        #[cfg(feature = "spim-test")]
        {
            use cramium_hal::board::{SPIM_FLASH_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR};
            use cramium_hal::ifram::IframRange;
            use cramium_hal::iox::*;
            use cramium_hal::udma::*;
            use loader::swap::SPIM_FLASH_IFRAM_ADDR;

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

            // setup the I/O pins
            let mut iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
            let mut udma_global = GlobalConfig::new();
            let channel = cramium_hal::board::setup_memory_pins(&iox);
            udma_global.clock_off(PeriphId::from(channel));

            crate::println!("Configuring SPI channel: {:?}", channel);
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
                    Some(8),
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
                    IframRange::from_raw_parts(SPIM_RAM_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR, 4096 * 2),
                )
            };
            crate::println!("spim init done");

            crate::println!(
                "Flash RxBuf: {:x}[{:x}] / {:x}[{:x}]",
                flash_spim.rx_buf::<u8>().as_ptr() as usize,
                flash_spim.rx_buf::<u8>().len(),
                unsafe { flash_spim.rx_buf_phys::<u8>().as_ptr() as usize },
                unsafe { flash_spim.rx_buf_phys::<u8>().len() },
            );
            crate::println!(
                "Ram RxBuf: {:x}[{:x}] / {:x}[{:x}]",
                ram_spim.rx_buf::<u8>().as_ptr() as usize,
                ram_spim.rx_buf::<u8>().len(),
                unsafe { ram_spim.rx_buf_phys::<u8>().as_ptr() as usize },
                unsafe { ram_spim.rx_buf_phys::<u8>().len() }
            );

            // turn off QPI mode, in case it was set from a reboot in a bad state
            flash_spim.mem_qpi_mode(false);
            ram_spim.mem_qpi_mode(false);

            // sanity check: read ID
            crate::println!("read ID...");
            // getc();
            let flash_id = flash_spim.mem_read_id_flash();
            let ram_id = ram_spim.mem_read_id_ram();
            crate::println!("flash ID: {:x}", flash_id);
            crate::println!("ram ID: {:x}", ram_id);
            // density 18, memory type 20, mfg ID C2 ==> MX25L128833F
            // density 38, memory type 25, mfg ID C2 ==> MX25U12832F
            assert!(flash_id & 0xFF_FF_FF == 0x1820C2 || flash_id & 0xFF_FF_FF == 0x38_25_C2);
            // KGD 5D, mfg ID 9D; remainder of bits are part of the EID
            assert!((ram_id & 0xFF_FF == 0x5D9D) || (ram_id & 0xFF_FF == 0x559d));

            // setup FLASH
            //  - QE enable
            //  - dummy cycles = 8
            crate::println!("write SR...");
            // getc();
            flash_spim.mem_write_status_register(0b01_0000_00, 0b10_00_0_111);

            // set SPI devices to QPI mode
            // We expect a MX25L12833F (3.3V) on CS0
            // We expect a ISS66WVS4M8BLL (3.3V) on CS1
            // Both support QPI.
            crate::println!("set QPI mode...");
            // getc();
            flash_spim.mem_qpi_mode(true);
            ram_spim.mem_qpi_mode(true);

            crate::println!("read ID QPI mode...");
            // getc();
            let flash_id = flash_spim.mem_read_id_flash();
            let ram_id = ram_spim.mem_read_id_ram();
            crate::println!("QPI flash ID: {:x}", flash_id);
            crate::println!("QPI ram ID: {:x}", ram_id);
            // density 18, memory type 20, mfg ID C2 ==> MX25L128833F
            // density 38, memory type 25, mfg ID C2 ==> MX25U12832F
            assert!(flash_id & 0xFF_FF_FF == 0x1820C2 || flash_id & 0xFF_FF_FF == 0x38_25_C2);
            // KGD 5D, mfg ID 9D; remainder of bits are part of the EID
            assert!((ram_id & 0xFF_FF == 0x5D9D) || (ram_id & 0xFF_FF == 0x559d));

            let mut chk_buf = [0u8; 32];
            crate::println!("first read...");
            crate::println!("flash read");
            flash_spim.mem_read(0x0, &mut chk_buf, false);
            crate::println!("flash: {:x?}", chk_buf);
            ram_spim.mem_read(0x0, &mut chk_buf, false);
            crate::println!("RAM: {:x?}", chk_buf);
            for (i, d) in chk_buf.iter_mut().enumerate() {
                *d = i as u8;
            }
            crate::println!("ram write...");
            ram_spim.mem_ram_write(0x0, &chk_buf, false);
            chk_buf.fill(0);
            crate::println!("empty buf: {:x?}", chk_buf);

            crate::println!("ram read...");
            ram_spim.mem_read(0x0, &mut chk_buf, false);
            crate::println!("RAM checked: {:x?}", chk_buf);

            /*
            crate::println!("Press any key to start SPIM RAM write test");
            let test_blocks = 4;
            getc();
            let mut big_buf = [0u8; 4096];
            for offset in (0..0x1000 * test_blocks).step_by(0x1000) {
                let mut test_pat = TestPattern::new(Some(offset));
                for d in big_buf.chunks_mut(4) {
                    d.copy_from_slice(&test_pat.next().to_le_bytes());
                }
                ram_spim.mem_ram_write(offset, &mut big_buf);
                crate::println!(
                    "Offset: {:x} -> {:x?}..{:x?}",
                    offset,
                    &big_buf[..16],
                    &big_buf[big_buf.len() - 16..]
                );
            }

            crate::println!("Press any key to start SPIM RAM read test");
            getc();
            let mut failures = 0;
            use core::convert::TryInto;
            for offset in (0..0x1000 * test_blocks).step_by(0x1000) {
                let mut test_pat = TestPattern::new(Some(offset));
                ram_spim.mem_read(offset, &mut big_buf);
                crate::println!(
                    "Offset: {:x} -> {:x?}..{:x?}",
                    offset,
                    &big_buf[..16],
                    &big_buf[big_buf.len() - 16..]
                );
                for d in big_buf.chunks(4) {
                    let val = u32::from_le_bytes(d.try_into().unwrap());
                    let expected = test_pat.next();
                    if val != expected {
                        failures += 1;
                    }
                }
            }
            crate::println!("total failures: {}", failures);
            crate::println!("SPIM ram test done; press any key to continue...");
            getc();
            */
            /*
            crate::println!("looping around, turning off QPI mode!");
            udma_uart.read(&mut rx_buf[..1]);
            flash_spim.mem_qpi_mode(false);
            ram_spim.mem_qpi_mode(false);
            */
        }

        // receive characters -- print them back. just to prove that this works. no other reason than that.
        for _ in 0..4 {
            udma_uart.read(&mut rx_buf[..1]);
            const DBG_MSG: &'static str = "Got: ";
            udma_uart.write(&DBG_MSG.as_bytes());
            rx_buf[1] = '\n' as u32 as u8;
            rx_buf[2] = '\r' as u32 as u8;
            udma_uart.write(&rx_buf);
        }

        // now wait for some interrupt-driven receive
        #[cfg(feature = "irq-test")]
        {
            irq_setup();
            let mut _c: u8 = 0;
            // this sets us up for async reads
            let should_be_zero = udma_uart.read_async(&mut _c);
            crate::println!("should_be_zero: {}", should_be_zero);
            crate::println!("Waiting for async hits...");
            NUM_RX.store(0, core::sync::atomic::Ordering::SeqCst);
            let mut last_rx = 0;
            let mut last_pending = 0;
            let irqarray5 = CSR::new(utra::irqarray5::HW_IRQARRAY5_BASE as *mut u32);
            crate::println!("irqarray5 enable: {:x}", irqarray5.r(utra::irqarray5::EV_ENABLE));
            loop {
                let cur_rx = NUM_RX.load(core::sync::atomic::Ordering::SeqCst);
                if cur_rx != last_rx {
                    crate::println!("Got async event {}", cur_rx);
                    last_rx = cur_rx;
                }
                if cur_rx > 4 {
                    break;
                }
                let pending = irqarray5.r(utra::irqarray5::EV_PENDING);
                if pending != last_pending {
                    crate::println!("pending: {:x}", pending);
                    last_pending = pending;
                }
            }
        }
    }

    #[cfg(feature = "trng-test")]
    {
        let mut csr = CSR::new(utralib::utra::trng::HW_TRNG_BASE as *mut u32);
        // assume: glbl_csr is already setup above, turning on clocks and setting up FIFOs

        csr.wo(utra::trng::SFR_CRSRC, 0xffff);
        csr.wo(utra::trng::SFR_CRANA, 0xffff);
        csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN0, 0xffff_ffff);
        csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN1, 0xffff_ffff);
        csr.wo(utra::trng::SFR_PP, 0xf805); // postproc
        csr.wo(utra::trng::SFR_OPT, 0); // opt

        loop {
            while csr.r(utra::trng::SFR_SR) & 0x100_0000 == 0 {}
            crate::println!("trng: {:x}", csr.r(utra::trng::SFR_BUF));
        }

        /*
        csr.wo(utra::trng::SFR_AR_GEN, 0xA5);
        csr.wo(utra::trng::SFR_CRSRC, 0xfff);
        csr.wo(utra::trng::SFR_CRANA, 0xf0f);
        csr.wo(utra::trng::SFR_PP, 0x1);
        csr.wo(utra::trng::SFR_OPT, 0xff);
        csr.wo(utra::trng::SFR_AR_GEN, 0x5A);
        */

        fn trng_start(csr: &mut CSR<u32>) { csr.wo(utra::trng::SFR_AR_GEN, 0x5A); }
        fn trng_stop(csr: &mut CSR<u32>) { csr.wo(utra::trng::SFR_AR_GEN, 0xA5); }
        fn trng_clock_enable(glbl_csr: &mut CSR<u32>) {
            glbl_csr.wo(utra::sce_glbsfr::SFR_SUBEN, 0xff);
            glbl_csr.wo(utra::sce_glbsfr::SFR_FFEN, 0x30);
        };
        fn trng_clock_disable(glbl_csr: &mut CSR<u32>) {
            glbl_csr.wo(utra::sce_glbsfr::SFR_SUBEN, 0x00);
            glbl_csr.wo(utra::sce_glbsfr::SFR_FFEN, 0x00);
        };
        fn trng_init(csr: &mut CSR<u32>) {
            csr.wo(utra::trng::SFR_CRSRC, 0xFFFF);
            csr.wo(utra::trng::SFR_CRANA, 0xFFFF);
            csr.wo(utra::trng::SFR_OPT, 0x10020);
            csr.wo(utra::trng::SFR_PP, 0x6801);
        }
        fn trng_continuous_prepare(csr: &mut CSR<u32>, glbl_csr: &mut CSR<u32>) {
            trng_stop(csr);
            glbl_csr.wo(utra::sce_glbsfr::SFR_FFCLR, 0xff05);
            csr.wo(utra::trng::SFR_CRSRC, 0xFFFF);
            csr.wo(utra::trng::SFR_CRANA, 0xFFFF);
            csr.wo(utra::trng::SFR_OPT, 0x10040);
            csr.wo(utra::trng::SFR_PP, 0xf821);
            trng_start(csr);
        }
    }
    #[cfg(feature = "usb-test")]
    {
        udma_uart.write("USB basic test...\n\r".as_bytes());
        let csr =
            cramium_hal::usb::compat::AtomicCsr::new(cramium_hal::usb::utra::CORIGINE_USB_BASE as *mut u32);
        let irq_csr =
            cramium_hal::usb::compat::AtomicCsr::new(utralib::utra::irqarray1::HW_IRQARRAY1_BASE as *mut u32);
        // safety: this is safe because we are in machine mode, and vaddr/paddr always pairs up
        let mut usb = unsafe {
            cramium_hal::usb::driver::CorigineUsb::new(
                0, // is dummy in no-std
                0, // is dummy in no-std
                cramium_hal::usb::driver::CRG_UDC_MEMBASE,
                csr,
                irq_csr,
            )
        };
        usb.reset();
        let mut idle_timer = 0;
        let mut vbus_on = false;
        let mut vbus_on_count = 0;
        let mut in_u0 = false;
        let mut last_sc = 0;
        loop {
            let next_sc = csr.r(cramium_hal::usb::utra::PORTSC);
            if last_sc != next_sc {
                last_sc = next_sc;
                crate::println!("**** SC update {:x?}", cramium_hal::usb::driver::PortSc(next_sc));
                /*
                if cramium_hal::usb::driver::PortSc(next_sc).pr() {
                    crate::println!("  >>reset<<");
                    usb.start();
                    in_u0 = false;
                    vbus_on_count = 0;
                }
                */
            }
            let event = usb.udc_handle_interrupt();
            if event == cramium_hal::usb::driver::CrgEvent::None {
                idle_timer += 1;
            } else {
                // crate::println!("*Event {:?} at {}", event, idle_timer);
                idle_timer = 0;
            }

            if !vbus_on && vbus_on_count == 4 {
                crate::println!("*Vbus on");
                usb.reset();
                usb.init();
                usb.start();
                vbus_on = true;
                in_u0 = false;

                let irq1 = irq_csr.r(utralib::utra::irqarray1::EV_PENDING);
                crate::println!("irq1: {:x}, status: {:x}", irq1, csr.r(cramium_hal::usb::utra::USBSTS));
                irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, irq1);
                // restore this to go on to boot
                // break;
            } else if usb.pp() && !vbus_on {
                vbus_on_count += 1;
                crate::println!("*Vbus_on_count: {}", vbus_on_count);
                // mdelay(100);
            } else if !usb.pp() && vbus_on {
                crate::println!("*Vbus off");
                usb.stop();
                usb.reset();
                vbus_on_count = 0;
                vbus_on = false;
                in_u0 = false;
            } else if in_u0 && vbus_on {
                // usb.udc_handle_interrupt();
                // TODO
            } else if usb.ccs() && vbus_on {
                // usb.print_status(usb.csr.r(cramium_hal::usb::utra::PORTSC));
                crate::println!("*Enter U0");
                in_u0 = true;
                let irq1 = irq_csr.r(utralib::utra::irqarray1::EV_PENDING);
                // usb.print_status(csr.r(cramium_hal::usb::utra::PORTSC));
                irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, irq1);
            }
        }
    }

    /*
    let aoc_base = utralib::CSR::new(0x4006_0000 as *mut u32);
    unsafe {
        for i in 0..0x50 / 4 {
            crate::println!("aoc[{:x}]: {:x}", i * 4, aoc_base.base().add(i).read_volatile());
        }
        let pmu_cr = aoc_base.base().add(4).read_volatile();
        crate::println!("pmu_cr: {:x}", pmu_cr);
        aoc_base.base().add(4).write_volatile(pmu_cr & !1);
        crate::println!("pmu_cr upd: {:x}", aoc_base.base().add(4).read_volatile());
    }
    */
    // udma_uart.write("Press any key to continue...".as_bytes());
    // getc();
    udma_uart.write(b"\n\rBooting!\n\r");

    perclk
}

#[cfg(feature = "clock-tests")]
unsafe fn clock_tests(udma_uart: &mut cramium_hal::udma::Uart) {
    // this test hangs, because we don't have an interrupt waker to leave WFI at this point
    if false {
        udma_uart.write("\n\rPress a key to go to WFI...".as_bytes());
        getc();
        crate::println!("Entering WFI");
        unsafe {
            core::arch::asm!("wfi");
        }
        crate::println!("Exited WFI");
    }

    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    // switch to osc-only mode
    if true {
        crate::println!("Press a key to go to OSC-only...");
        getc();
        daric_cgu.add(sysctrl::SFR_CGUSEL1.offset()).write_volatile(1); // 0: RC, 1: XTAL
        daric_cgu.add(sysctrl::SFR_CGUFSCR.offset()).write_volatile(48); // external crystal is 48MHz
        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

        // switch to OSC
        daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(0); // clktop sel, 0:clksys, 1:clkpll0
        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit

        crate::println!("OSC-only now. Press any key to turn off PLL...");
        daric_cgu.add(sysctrl::SFR_IPCEN.offset()).write_volatile(0);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
        getc();
        crate::println!("PLL off");
        getc();
        crate::println!("Dividers down");
        // conservative dividers
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7f1f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x7f1f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x3f1f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x1f1f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0f1f);
        // ungate all clocks
        daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x1);
        daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x0);
        daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x0);
        daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x0);
        // commit clocks
        daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        getc();
    }

    if false {
        crate::println!("Press a key to lower clock from 800MHz -> 400MHz...");
        getc();
        init_clock_asic(400_000_000);
        crate::println!("Press a key to lower clock from 400MHz -> 200MHz...");
        getc();
        init_clock_asic(200_000_000);
        getc();
    }
    crate::println!("Leaving clock test");
}

#[cfg(feature = "platform-tests")]
pub mod duart {
    pub const UART_DOUT: utralib::Register = utralib::Register::new(0, 0xff);
    pub const UART_DOUT_DOUT: utralib::Field = utralib::Field::new(8, 0, UART_DOUT);
    pub const UART_CTL: utralib::Register = utralib::Register::new(1, 1);
    pub const UART_CTL_EN: utralib::Field = utralib::Field::new(1, 0, UART_CTL);
    pub const UART_BUSY: utralib::Register = utralib::Register::new(2, 1);
    pub const UART_BUSY_BUSY: utralib::Field = utralib::Field::new(1, 0, UART_BUSY);

    pub const HW_DUART_BASE: usize = 0x4004_2000;
}
#[cfg(feature = "platform-tests")]
struct Duart {
    csr: utralib::CSR<u32>,
}
#[cfg(feature = "platform-tests")]
impl Duart {
    pub fn new() -> Self {
        let mut duart_csr = utralib::CSR::new(duart::HW_DUART_BASE as *mut u32);
        duart_csr.wfo(duart::UART_CTL_EN, 1);
        Duart { csr: duart_csr }
    }

    pub fn putc(&mut self, ch: char) {
        while self.csr.rf(duart::UART_BUSY_BUSY) != 0 {
            // spin wait
        }
        // the code here bypasses a lot of checks to simulate very fast write cycles so
        // that the read waitback actually returns something other than not busy.

        // unsafe {(duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) }; // this line really ensures
        // we have to readback something, but it causes double-printing
        while unsafe { (duart::HW_DUART_BASE as *mut u32).add(2).read_volatile() } != 0 {
            // wait
        }
        unsafe { (duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) };
    }

    pub fn puts(&mut self, s: &str) {
        for c in s.as_bytes() {
            self.putc(*c as char);
        }
    }
}
#[cfg(feature = "platform-tests")]
fn test_duart() {
    // println!("Duart test\n");
    let mut duart = Duart::new();
    loop {
        duart.puts("hello world\n");
    }
}

#[cfg(feature = "platform-tests")]
pub fn platform_tests() { test_duart(); }

// returns the actual per_clk
#[cfg(not(feature = "simulation-only"))]
pub unsafe fn init_clock_asic(freq_hz: u32) -> u32 {
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    const UNIT_MHZ: u32 = 1000 * 1000;
    const PFD_F_MHZ: u32 = 16;
    const FREQ_0: u32 = 16 * UNIT_MHZ;
    const FREQ_OSC_MHZ: u32 = 48; // Actually 48MHz
    const M: u32 = FREQ_OSC_MHZ / PFD_F_MHZ; //  - 1;  // OSC input was 24, replace with 48

    const TBL_Q: [u16; 7] = [
        // keep later DIV even number as possible
        0x7777, // 16-32 MHz
        0x7737, // 32-64
        0x3733, // 64-128
        0x3313, // 128-256
        0x3311, // 256-512 // keep ~ 100MHz
        0x3301, // 512-1024
        0x3301, // 1024-1600
    ];
    const TBL_MUL: [u32; 7] = [
        64, // 16-32 MHz
        32, // 32-64
        16, // 64-128
        8,  // 128-256
        4,  // 256-512
        2,  // 512-1024
        2,  // 1024-1600
    ];

    // this block might belong at the top, in particular, configuring the dividers prevents stuff
    // from being overclocked when the PLL comes live; but for now we are debugging other stuff
    let perclk = {
        // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
        // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3fff); // fclk

        // Hits a 8:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk
        // TODO: derive this from the actual perclk value, based on the values in the lookup table below...
        let perclk = if freq_hz > 400_000_000 {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x07_ff_ff);
            freq_hz / 8
        } else {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
            freq_hz / 4
        };

        /*
            perclk fields:  min-cycle-lp | min-cycle | fd-lp | fd
            clkper fd
                0xff :   Fperclk = Fclktop/2
                0x7f:   Fperclk = Fclktop/4
                0x3f :   Fperclk = Fclktop/8
                0x1f :   Fperclk = Fclktop/16
                0x0f :   Fperclk = Fclktop/32
                0x07 :   Fperclk = Fclktop/64
                0x03:   Fperclk = Fclktop/128
                0x01:   Fperclk = Fclktop/256

            min cycle of clktop, F means frequency
            Fperclk  Max = Fperclk/(min cycle+1)*2
        */

        // turn off gates
        daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xff);
        daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xff);
        daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xff);
        daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xff);
        crate::println!("bef gates set");
        for _ in 0..100 {
            crate::print!("*");
        }
        crate::println!(".");
        // commit dividers
        daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        crate::println!("gates set");
        for _ in 0..100 {
            crate::print!("-");
        }
        crate::println!(".");
        perclk
    };

    for _ in 0..100 {
        crate::print!("1");
    }
    crate::println!(".");

    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    for _ in 0..100 {
        crate::print!("2");
    }
    crate::println!(".");

    if freq_hz < 1000000 {
        cgu.wo(sysctrl::SFR_IPCOSC, freq_hz);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
    }
    // switch to OSC
    // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    // commit
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    for _ in 0..100 {
        crate::print!("3");
    }
    crate::println!(".");

    if freq_hz < 1000000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        for _ in 0..100 {
            crate::print!("4");
        }
        crate::println!(".");

        // PD PLL
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
        // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 1");

        n_fxp24 = (((freq_hz as u64) << 24) * TBL_MUL[f16mhz_log2 as usize] as u64
            + PFD_F_MHZ as u64 * UNIT_MHZ as u64 / 2)
            / (PFD_F_MHZ as u64 * UNIT_MHZ as u64); // rounded
        let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;

        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
        // DARIC_IPC->pll_f = n_frac | ((0 == n_frac) ? 0 : (1UL << 24));
        cgu.wo(sysctrl::SFR_IPCPLLF, n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
        // DARIC_IPC->pll_q = TBL_Q[f16MHzLog2]; // ?? TODO select DIV for VCO freq
        cgu.wo(sysctrl::SFR_IPCPLLQ, TBL_Q[f16mhz_log2 as usize] as u32);
        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        //DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        // DARIC_IPC->ipc = (1 << 6) | (2 << 3) | (3);
        cgu.wo(sysctrl::SFR_IPCCR, (1 << 6) | (2 << 3) | (3));
        // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);

        // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 2");

        // TODO wait/poll lock status?
        // DARIC_CGU->cgusel0 = 1; // clktop sel, 0:clksys, 1:clkpll0
        cgu.wo(sysctrl::SFR_CGUSEL0, 1);
        // __DSB();
        // DARIC_CGU->cguset = 0x32; // commit
        cgu.wo(sysctrl::SFR_CGUSET, 0x32);
        crate::println!("clocks set");
    }
    crate::println!(
        "mn {:x}, q{:x}",
        (0x400400a0 as *const u32).read_volatile(),
        (0x400400a8 as *const u32).read_volatile()
    );

    crate::println!("fsvalid: {}", daric_cgu.add(sysctrl::SFR_CGUFSVLD.offset()).read_volatile());
    let clk_desc: [(&'static str, u32, usize); 8] = [
        ("fclk", 16, 0x40 / size_of::<u32>()),
        ("pke", 0, 0x40 / size_of::<u32>()),
        ("ao", 16, 0x44 / size_of::<u32>()),
        ("aoram", 0, 0x44 / size_of::<u32>()),
        ("osc", 16, 0x48 / size_of::<u32>()),
        ("xtal", 0, 0x48 / size_of::<u32>()),
        ("pll0", 16, 0x4c / size_of::<u32>()),
        ("pll1", 0, 0x4c / size_of::<u32>()),
    ];
    for (name, shift, offset) in clk_desc {
        let fsfreq = (daric_cgu.add(offset).read_volatile() >> shift) & 0xffff;
        crate::println!("{}: {} MHz", name, fsfreq);
    }

    crate::println!("PLL configured to {} MHz; perclk {}", freq_hz / 1_000_000, perclk);
    perclk
}

#[allow(dead_code)]
fn fsfreq_to_hz(fs_freq: u32) -> u32 { (fs_freq * (48_000_000 / 32)) / 1_000_000 }

#[allow(dead_code)]
fn fsfreq_to_hz_32(fs_freq: u32) -> u32 { (fs_freq * (32_000_000 / 32)) / 1_000_000 }

#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
/// Used mainly for debug breaks. Not used in every configuration.
pub fn getc() -> char {
    let uart_buf_addr = loader::UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    let mut rx_buf = [0u8; 1];
    udma_uart.read(&mut rx_buf);
    char::from_u32(rx_buf[0] as u32).unwrap_or(' ')
}

#[allow(dead_code)]
pub struct TestPattern {
    x: u32,
}
#[allow(dead_code)]
impl TestPattern {
    pub fn new(seed: Option<u32>) -> Self { Self { x: seed.unwrap_or(0) } }

    /// from https://github.com/skeeto/hash-prospector
    pub fn next(&mut self) -> u32 {
        if self.x == 0 {
            self.x += 1;
        }
        self.x ^= self.x >> 17;
        self.x *= 0xed5ad4bb;
        self.x ^= self.x >> 11;
        self.x *= 0xac4c1b51;
        self.x ^= self.x >> 15;
        self.x *= 0x31848bab;
        self.x ^= self.x >> 14;
        return self.x;
    }
}

#[cfg(feature = "irq-test")]
static NUM_RX: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[cfg(feature = "irq-test")]
pub fn irq_setup() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            // Set trap handler
            "la   t0, _start_trap", // this first one forces the nop sled symbol to be generated
            "la   t0, _start_trap_aligned", // this is the actual target
            "csrw mtvec, t0",
        );
    }

    // enable IRQ handling
    riscv::register::vexriscv::mim::write(0x0); // first make sure everything is disabled, so we aren't OR'ing in garbage
    // this will set the IRQ bit for the uart bank as part of the new() function
    let mut uart_irq = cramium_hal::udma::UartIrq::new();
    uart_irq.rx_irq_ena(udma::UartChannel::Uart1, true);
    // the actual handler is hard-coded below :'( but this is just a quick and dirty test so meh?

    let mut irqarray5 = CSR::new(utra::irqarray5::HW_IRQARRAY5_BASE as *mut u32);
    irqarray5.wo(utra::irqarray5::EV_PENDING, irqarray5.r(utra::irqarray5::EV_PENDING));

    // must enable external interrupts on the CPU for any of the above to matter
    unsafe { riscv::register::mie::set_mext() };

    crate::println!(
        "mie: {:x}, mim: {:x}",
        riscv::register::mie::read().bits(),
        riscv::register::vexriscv::mim::read()
    );
}

#[export_name = "_start_trap"]
#[inline(never)]
#[cfg(feature = "irq-test")]
pub unsafe extern "C" fn _start_trap() -> ! {
    loop {
        // install a NOP sled before _start_trap() until https://github.com/rust-lang/rust/issues/82232 is stable
        #[rustfmt::skip]
        core::arch::asm!(
            "nop",
            "nop",
        );
        #[export_name = "_start_trap_aligned"]
        pub unsafe extern "C" fn _start_trap_aligned() {
            #[rustfmt::skip]
            core::arch::asm!(
                "csrw        mscratch, sp",
                "li          sp, 0x61008000", // a random location that we corrupt for testing routine
                "sw       x1, 0*4(sp)",
                // Skip SP for now
                "sw       x3, 2*4(sp)",
                "sw       x4, 3*4(sp)",
                "sw       x5, 4*4(sp)",
                "sw       x6, 5*4(sp)",
                "sw       x7, 6*4(sp)",
                "sw       x8, 7*4(sp)",
                "sw       x9, 8*4(sp)",
                "sw       x10, 9*4(sp)",
                "sw       x11, 10*4(sp)",
                "sw       x12, 11*4(sp)",
                "sw       x13, 12*4(sp)",
                "sw       x14, 13*4(sp)",
                "sw       x15, 14*4(sp)",
                "sw       x16, 15*4(sp)",
                "sw       x17, 16*4(sp)",
                "sw       x18, 17*4(sp)",
                "sw       x19, 18*4(sp)",
                "sw       x20, 19*4(sp)",
                "sw       x21, 20*4(sp)",
                "sw       x22, 21*4(sp)",
                "sw       x23, 22*4(sp)",
                "sw       x24, 23*4(sp)",
                "sw       x25, 24*4(sp)",
                "sw       x26, 25*4(sp)",
                "sw       x27, 26*4(sp)",
                "sw       x28, 27*4(sp)",
                "sw       x29, 28*4(sp)",
                "sw       x30, 29*4(sp)",
                "sw       x31, 30*4(sp)",
                // Save MEPC
                "csrr        t0, mepc",
                "sw       t0, 31*4(sp)",
                // Finally, save SP
                "csrr        t0, mscratch",
                "sw          t0, 1*4(sp)",
                // Restore a default stack pointer
                "li          sp, 0x6100A000", // more random locations to corrupt
                // Note that registers $a0-$a7 still contain the arguments
                "j           _start_trap_rust",
            );
        }
        _start_trap_aligned();
        #[rustfmt::skip]
        core::arch::asm!(
            "nop",
            "nop",
        );
    }
}

#[export_name = "_resume_context"]
#[inline(never)]
#[cfg(feature = "irq-test")]
pub unsafe extern "C" fn _resume_context(registers: u32) -> ! {
    #[rustfmt::skip]
    core::arch::asm!(
        "move        sp, {registers}",

        "lw        x1, 0*4(sp)",
        // Skip SP for now
        "lw        x3, 2*4(sp)",
        "lw        x4, 3*4(sp)",
        "lw        x5, 4*4(sp)",
        "lw        x6, 5*4(sp)",
        "lw        x7, 6*4(sp)",
        "lw        x8, 7*4(sp)",
        "lw        x9, 8*4(sp)",
        "lw        x10, 9*4(sp)",
        "lw        x11, 10*4(sp)",
        "lw        x12, 11*4(sp)",
        "lw        x13, 12*4(sp)",
        "lw        x14, 13*4(sp)",
        "lw        x15, 14*4(sp)",
        "lw        x16, 15*4(sp)",
        "lw        x17, 16*4(sp)",
        "lw        x18, 17*4(sp)",
        "lw        x19, 18*4(sp)",
        "lw        x20, 19*4(sp)",
        "lw        x21, 20*4(sp)",
        "lw        x22, 21*4(sp)",
        "lw        x23, 22*4(sp)",
        "lw        x24, 23*4(sp)",
        "lw        x25, 24*4(sp)",
        "lw        x26, 25*4(sp)",
        "lw        x27, 26*4(sp)",
        "lw        x28, 27*4(sp)",
        "lw        x29, 28*4(sp)",
        "lw        x30, 29*4(sp)",
        "lw        x31, 30*4(sp)",

        // Restore SP
        "lw        x2, 1*4(sp)",
        "mret",
        registers = in(reg) registers,
    );
    loop {}
}

/// Just handles specific traps for testing CPU interactions. Doesn't do anything useful with the traps.
#[export_name = "_start_trap_rust"]
#[cfg(feature = "irq-test")]
pub extern "C" fn trap_handler(
    _a0: usize,
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> ! {
    use riscv::register::{mcause, mie, vexriscv::mip};
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(
            utra::udma_uart_1::HW_UDMA_UART_1_BASE,
            loader::UART_IFRAM_ADDR,
            loader::UART_IFRAM_ADDR,
        )
    };

    let mc: mcause::Mcause = mcause::read();
    if mc.bits() == 0x8000_0009 {
        // external interrupt. find out which ones triggered it, and clear the source.
        let irqs_pending = mip::read();
        if (irqs_pending & (1 << utra::irqarray5::IRQARRAY5_IRQ)) != 0 {
            let mut irqarray5 = CSR::new(utra::irqarray5::HW_IRQARRAY5_BASE as *mut u32);

            let pending = irqarray5.r(utra::irqarray5::EV_PENDING);
            let mut c: u8 = 0;
            let should_be_one = udma_uart.read_async(&mut c);
            let mut buf = [0u8; 16];
            udma_uart.write("async_rx ".as_bytes());
            buf[0] = should_be_one as u8 + '0' as u32 as u8;
            buf[1] = ':' as u32 as u8;
            buf[2] = c;
            udma_uart.write(&buf[..3]);
            NUM_RX.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
            // clear all pending
            irqarray5.wo(utra::irqarray5::EV_PENDING, pending);
        }
    } else {
        udma_uart.write("Unrecognized interrupt case".as_bytes());
    }

    // re-enable interrupts
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            "csrr        t0, mstatus",
            "ori         t0, t0, 3",
            "csrw        mstatus, t0",
        );
    }
    unsafe { mie::set_mext() };
    unsafe { _resume_context(0x61008000u32) }; // this is the scratch page used in the assembly routine above
}

#[allow(dead_code)]
pub fn log_2(mut value: u32) -> u32 {
    let mut result = 0;

    // Shift right until we find the position of the highest set bit
    while value > 1 {
        value >>= 1;
        result += 1;
    }

    result
}

#[allow(dead_code)]
/// Direct translation of the C code
pub unsafe fn init_clock_asic_c(freq_hz: u32, duty_sram: u32) -> u32 {
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    const UNIT_MHZ: u32 = 1000u32 * 1000u32;
    const PFD_F_MHZ: u32 = 16;
    const FREQ_0: u32 = 16u32 * UNIT_MHZ;
    const FREQ_OSC_MHZ: u32 = 48; // Actually 48MHz
    const M: u32 = FREQ_OSC_MHZ / PFD_F_MHZ; //  - 1;  // OSC input was 24, replace with 48

    const TBL_Q: [u16; 7] = [
        // keep later DIV even number as possible
        0x7777, // 16-32 MHz
        0x7737, // 32-64
        0x3733, // 64-128
        0x3313, // 128-256
        0x3311, // 256-512 // keep ~ 100MHz
        0x3301, // 512-1024
        0x3301, // 1024-1600
    ];
    const TBL_MUL: [u32; 7] = [
        64, // 16-32 MHz
        32, // 32-64
        16, // 64-128
        8,  // 128-256
        4,  // 256-512
        2,  // 512-1024
        2,  // 1024-1600
    ];

    if (0 == (daric_cgu.add(sysctrl::SFR_IPCPLLMN.offset()).read_volatile() & 0x0001F000))
        || (0 == (daric_cgu.add(sysctrl::SFR_IPCPLLMN.offset()).read_volatile() & 0x00000fff))
    {
        // for SIM, avoid div by 0 if unconfigurated
        // , default VCO 48MHz / 48 * 1200 = 1.2GHz
        // TODO magic numbers
        daric_cgu
            .add(sysctrl::SFR_IPCPLLMN.offset())
            .write_volatile(((M << 12) & 0x0001F000) | ((1200) & 0x00000fff));
        daric_cgu.add(sysctrl::SFR_IPCPLLF.offset()).write_volatile(0); // ??
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    // TODO select int/ext osc/xtal
    daric_cgu.add(sysctrl::SFR_CGUSEL1.offset()).write_volatile(1); // 0: RC, 1: XTAL
    daric_cgu.add(sysctrl::SFR_CGUFSCR.offset()).write_volatile(FREQ_OSC_MHZ); // external crystal is 48MHz
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    if freq_hz < 1000000 {
        daric_cgu.add(sysctrl::SFR_IPCOSC.offset()).write_volatile(freq_hz);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    // switch to OSC
    daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(0); // clktop sel, 0:clksys, 1:clkpll0
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    if freq_hz < 1000000 {
    } else {
        let f16_mhz_log2: usize = log_2(freq_hz / FREQ_0) as usize;

        // PD PLL
        daric_cgu
            .add(sysctrl::SFR_IPCLPEN.offset())
            .write_volatile(daric_cgu.add(sysctrl::SFR_IPCLPEN.offset()).read_volatile() | 0x02);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }

        let n_fxp24: u64 = (((freq_hz as u64) << 24u64) * TBL_MUL[f16_mhz_log2] as u64
            + PFD_F_MHZ as u64 * UNIT_MHZ as u64 / 2u64)
            / (PFD_F_MHZ as u64 * UNIT_MHZ as u64); // rounded
        let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;

        daric_cgu
            .add(sysctrl::SFR_IPCPLLMN.offset())
            .write_volatile(((M << 12) & 0x0001F000) | ((n_fxp24 >> 24) & 0x00000fff) as u32); // 0x1F598; // ??
        daric_cgu
            .add(sysctrl::SFR_IPCPLLF.offset())
            .write_volatile(n_frac | if 0 == n_frac { 0 } else { 1 << 24 }); // ??
        daric_cgu.add(sysctrl::SFR_IPCPLLQ.offset()).write_volatile(TBL_Q[f16_mhz_log2] as u32); // ?? TODO select DIV for VCO freq

        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        //DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        daric_cgu.add(sysctrl::SFR_IPCCR.offset()).write_volatile((1 << 6) | (2 << 3) | (3));
        // daric_cgu.add(sysctrl::SFR_IPCCR.offset()).write_volatile((3 << 6) | (5 << 3) | (5));
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        daric_cgu
            .add(sysctrl::SFR_IPCLPEN.offset())
            .write_volatile(daric_cgu.add(sysctrl::SFR_IPCLPEN.offset()).read_volatile() & !0x02);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        //printf("read reg a0 : %08" PRIx32"\n", *((volatile uint32_t* )0x400400a0));
        //printf("read reg a4 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a4));
        //printf("read reg a8 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a8));
        crate::println!("PLL switchover");
        // TODO wait/poll lock status?
        daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(1); // clktop sel, 0:clksys, 1:clkpll0
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        crate::println!("PLL switchover done");

        // printf ("    MN: 0x%05x, F: 0x%06x, Q: 0x%04x\n",
        //     DARIC_IPC->pll_mn, DARIC_IPC->pll_f, DARIC_IPC->pll_q);
        // printf ("    LPEN: 0x%01x, OSC: 0x%04x, BIAS: 0x%04x,\n",
        //     DARIC_IPC->lpen, DARIC_IPC->osc, DARIC_IPC->ipc);
    }

    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7fff); // CPU
    if 0 == duty_sram {
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
    } else {
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(duty_sram);
    }
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    // UDMACORE->CFG_CG = 0xffffffff; //everything on
    // core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    100_000_000 // bodge for now
}

#[cfg(all(feature = "verilator-only", not(feature = "cramium-mpw")))]
pub fn coreuser_config() {
    // configure coruser signals. Specific to NTO.
    use utra::coreuser::*;
    crate::println!("coreuser setup...");
    let mut coreuser = CSR::new(utra::coreuser::HW_COREUSER_BASE as *mut u32);
    // set to 0 so we can safely mask it later on
    coreuser.wo(USERVALUE, 0);
    coreuser.rmwf(utra::coreuser::USERVALUE_DEFAULT, 3);
    let trusted_asids = [(1, 0), (2, 0), (3, 1), (4, 2), (1, 0), (1, 0), (1, 0), (1, 0)];
    let asid_fields = [
        (utra::coreuser::MAP_LO_LUT0, utra::coreuser::USERVALUE_USER0),
        (utra::coreuser::MAP_LO_LUT1, utra::coreuser::USERVALUE_USER1),
        (utra::coreuser::MAP_LO_LUT2, utra::coreuser::USERVALUE_USER2),
        (utra::coreuser::MAP_LO_LUT3, utra::coreuser::USERVALUE_USER3),
        (utra::coreuser::MAP_HI_LUT4, utra::coreuser::USERVALUE_USER4),
        (utra::coreuser::MAP_HI_LUT5, utra::coreuser::USERVALUE_USER5),
        (utra::coreuser::MAP_HI_LUT6, utra::coreuser::USERVALUE_USER6),
        (utra::coreuser::MAP_HI_LUT7, utra::coreuser::USERVALUE_USER7),
    ];
    for (&(asid, value), (map_field, uservalue_field)) in trusted_asids.iter().zip(asid_fields) {
        coreuser.rmwf(map_field, asid);
        coreuser.rmwf(uservalue_field, value);
    }
    coreuser.rmwf(CONTROL_INVERT_PRIV, 1);
    coreuser.rmwf(CONTROL_ENABLE, 1);

    // turn off updates
    coreuser.wo(utra::coreuser::PROTECT, 1);
    crate::println!("coreuser locked!");
}
