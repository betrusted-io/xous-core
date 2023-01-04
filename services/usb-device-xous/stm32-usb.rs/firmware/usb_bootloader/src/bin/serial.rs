//! CDC-ACM serial port example using cortex-m-rtfm.
#![no_main]
#![no_std]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_imports)]

use core::{
    panic::PanicInfo,
    sync::atomic::{self, Ordering},
    str::from_utf8_unchecked,
    ptr::{
        read_volatile,
        write_volatile,
    },
    convert::TryFrom,
    mem,
    ops::RangeInclusive,
};
use cortex_m::{
    interrupt,
    asm::*,
};    
use embedded_hal::digital::v2::OutputPin;
use rtfm::app;
use stm32f1xx_hal::{
    prelude::*,
    time::Hertz,
};
use stm32f1xx_hal::{
    usb::{
        Peripheral, 
        UsbBus, 
        UsbBusType,
    },
    pac::FLASH,
};
use usb_device::{
    bus,
    device::{ 
        UsbDevice, 
        UsbDeviceBuilder, 
        UsbVidPid,
    },
    UsbError,
};
use usbd_serial::{CdcAcmClass, SerialPort, USB_CLASS_CDC};
use itm_logger::*;
use usb_bootloader::hardware_extra::*;

// VID and PID are from dapboot bluepill bootloader
const USB_VID: u16 = 0x1209; 
const USB_PID: u16 = 0xDB42;
const USB_CLASS_MISCELLANEOUS: u8 =  0xEF;

#[cfg(feature = "itm")] 
use cortex_m::{iprintln, peripheral::ITM};

#[app(device = stm32f1xx_hal::stm32, peripherals = true)]
const APP: () = {
    struct Resources {
        usb_dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
    }

    #[init]
    fn init(cx: init::Context) -> init::LateResources {
        static mut USB_BUS: Option<bus::UsbBusAllocator<UsbBusType>> = None;

        #[cfg(feature = "itm")]
        {        
            update_tpiu_baudrate(8_000_000, ITM_BAUD_RATE).expect("Failed to reset TPIU baudrate");
            logger_init();
        }

        info!("ITM reset ok.");

        let mut flash = cx.device.FLASH.constrain();
        let mut rcc = cx.device.RCC.constrain();

        let clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(48.mhz())
            .pclk1(24.mhz())
            .freeze(&mut flash.acr);

        #[cfg(feature = "itm")]
        {
            let sysclk: Hertz = clocks.sysclk().into();
            update_tpiu_baudrate(sysclk.0, ITM_BAUD_RATE).expect("Failed to reset TPIU baudrate");
        }

        assert!(clocks.usbclk_valid());

        let flash_kib = FlashSize::get().kibi_bytes();
        info!("Flash: {} KiB", flash_kib);

        
        let mut gpioa = cx.device.GPIOA.split(&mut rcc.apb2);

        // BluePill board has a pull-up resistor on the D+ line.
        // Pull the D+ pin down to send a RESET condition to the USB bus.
        // This forced reset is needed only for development, without it host
        // will not reset your device when you upload new firmware.
        let mut usb_dp = gpioa.pa12.into_push_pull_output(&mut gpioa.crh);
        usb_dp.set_low().unwrap();
        delay(clocks.sysclk().0 / 100);

        let usb_dm = gpioa.pa11;
        let usb_dp = usb_dp.into_floating_input(&mut gpioa.crh);

        let usb = Peripheral {
            usb: cx.device.USB,
            pin_dm: usb_dm,
            pin_dp: usb_dp,
        };

        *USB_BUS = Some(UsbBus::new(usb));

        let serial = SerialPort::new(USB_BUS.as_ref().unwrap());
        
        let serial_number = get_serial_number();
        info!("Serial number: {}", serial_number);

        let usb_dev = UsbDeviceBuilder::new(USB_BUS.as_ref().unwrap(), UsbVidPid(USB_VID, USB_PID))
            .manufacturer("Fake company")
            .product("Serial port")
            .serial_number(serial_number)
            .self_powered(true)
            .device_class(USB_CLASS_CDC)
            .build();

        init::LateResources { usb_dev, serial }
    }

    #[task(binds = USB_HP_CAN_TX, resources = [usb_dev, serial])]
    fn usb_tx(mut cx: usb_tx::Context) {
        usb_poll(&mut cx.resources.usb_dev, &mut cx.resources.serial);
    }

    #[task(binds = USB_LP_CAN_RX0, resources = [usb_dev, serial])]
    fn usb_rx0(mut cx: usb_rx0::Context) {
        usb_poll(&mut cx.resources.usb_dev, &mut cx.resources.serial);
    }
};

fn usb_poll<B: bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    serial: &mut SerialPort<'static, B>,
) {
    if !usb_dev.poll(&mut [serial]) {
        return;
    }

    let mut buf = [0; 64];

    match serial.read(&mut buf) {
        Ok(count) => {
            let _ = serial.write(&buf[..count]); 
        },
        Err(UsbError::WouldBlock) => {},
        Err(e) => info!("Err: {:?}", e),
    }
}


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