#![no_std]
#![no_main]

#[allow(unused_imports)]
use cortex_m::{asm, singleton};

use core::{
    panic::PanicInfo,
    sync::atomic::{self, Ordering},
};
use cortex_m::interrupt;
    
use stm32f1xx_hal::prelude::*;

use embedded_hal::digital::v2::OutputPin;

#[rtfm::app(device = stm32f1xx_hal::stm32, peripherals = true)]
const APP: () = {    
    #[init]
    fn init(cx: init::Context) {
        let device = cx.device;
        let mut rcc = device.RCC.constrain();
        let mut gpioc = device.GPIOC.split(&mut rcc.apb2);
        
        let mut led_usr = gpioc.pc13.into_push_pull_output(&mut gpioc.crh);
        
        loop {
            let _ = led_usr.set_high();
            for _ in 0..100000 { asm::nop() }
            let _ = led_usr.set_low();
            for _ in 0..100000 { asm::nop() }
        }
    }
};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    interrupt::disable();

    loop {
        // add some side effect to prevent this from turning into a UDF instruction
        // see rust-lang/rust#28728 for details
        atomic::compiler_fence(Ordering::SeqCst)
    }
}