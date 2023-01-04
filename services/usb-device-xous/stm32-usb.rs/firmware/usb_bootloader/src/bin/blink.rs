#![no_std]
#![no_main]

#[allow(unused_imports)]
use cortex_m::{asm, singleton};

use core::{
    panic::PanicInfo,
    sync::atomic::{self, Ordering},
};
use cortex_m::{
    interrupt,
    peripheral::Peripherals as CorePeripherals,
};

#[cfg(feature = "itm")] 
use cortex_m::{iprintln, peripheral::ITM};
    
use itm_logger::*;

use stm32f1xx_hal::{
    prelude::*,
    rcc::Clocks,
    pac::{
        DMA1,
        Peripherals as DevicePeripherals,
    },
    gpio::{
        ExtiPin,
        Output,
        PushPull,
        gpioc::*,
    },    
    adc::{
        Adc,
        AdcPayload,
        Continuous,
    },
    dma::{
        RxDma,
        CircBuffer,
        dma1::{
            C1 as DmaC1,
        },
    },
    delay::Delay,
};

use embedded_hal::digital::v2::OutputPin;

#[rtfm::app(device = stm32f1xx_hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        led_usr: PC13<Output<PushPull>>,
    }
    
    #[init]
    fn init(cx: init::Context) -> init::LateResources {
        asm::bkpt();
        let device = cx.device;
        let mut rcc = device.RCC.constrain();
        let mut flash = device.FLASH.constrain();
        let mut gpioc = device.GPIOC.split(&mut rcc.apb2);
        let mut afio = device.AFIO.constrain(&mut rcc.apb2);

        let mut led_usr = gpioc.pc13.into_push_pull_output(&mut gpioc.crh);
        
        loop {
            led_usr.set_high();
            for _ in 0..100000 { asm::nop() }
            led_usr.set_low();
            for _ in 0..100000 { asm::nop() }
        }
    
        init::LateResources {
            led_usr,
        }
    }

    #[idle]
    fn idle(_cx: idle::Context) -> ! {
        loop {
            asm::wfi();
        }
    }

    
    #[task(binds = TIM8_UP, resources = [])]
    fn tim8_up(cx: tim8_up::Context) {
        static mut COUNT: u16 = 0;

        
    }

};

#[panic_handler]
fn panic(
    #[cfg_attr(not(feature = "itm"), allow(unused_variables))]
    info: &PanicInfo
) -> ! {
    interrupt::disable();

    #[cfg(feature = "itm")]
    {
        let itm = unsafe { &mut *ITM::ptr() };
        let stim = &mut itm.stim[0];

        iprintln!(stim, "{}", info);
    }

    loop {
        // add some side effect to prevent this from turning into a UDF instruction
        // see rust-lang/rust#28728 for details
        atomic::compiler_fence(Ordering::SeqCst)
    }
}