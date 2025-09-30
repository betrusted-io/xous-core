use core::sync::atomic::{AtomicBool, Ordering::SeqCst};

use bao1x_api::*;
use bao1x_hal::iox::Iox;
use bao1x_hal::udma::UartIrq;
use bao1x_hal::{udma, udma::GlobalConfig};
use utralib::generated::*;

pub static USE_CONSOLE: AtomicBool = AtomicBool::new(false);

/// A trait for serial like drivers which allows reading from a source.
#[allow(dead_code)]
pub trait SerialRead {
    /// Read a single byte.
    fn getc(&mut self) -> Option<u8>;
}
pub struct Uart {}

#[allow(dead_code)]
impl Uart {
    pub fn putc(&self, c: u8) {
        if !USE_CONSOLE.load(SeqCst) {
            let base = utra::duart::HW_DUART_BASE as *mut u32;
            let mut uart = CSR::new(base);
            if uart.rf(utra::duart::SFR_CR_SFR_CR) == 0 {
                uart.wfo(utra::duart::SFR_CR_SFR_CR, 1);
            }
            while uart.r(utra::duart::SFR_SR) != 0 {}
            uart.wo(utra::duart::SFR_TXD, c as u32);
        } else {
            if crate::USB_CONNECTED.load(SeqCst) {
                critical_section::with(|cs| {
                    let mut queue = crate::USB_TX.borrow(cs).borrow_mut();
                    // arbitrary limit to avoid runaway memory allocation in the case that
                    // the host side doesn't have a terminal up and running
                    if queue.len() < 4096 {
                        queue.push_back(c);
                    }
                });
            } else {
                let buf: [u8; 1] = [c];
                let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
                #[cfg(feature = "bao1x-evb")]
                let mut udma_uart = unsafe {
                    // safety: this is safe to call, because we set up clock and events prior to calling new.
                    udma::Uart::get_handle(
                        utra::udma_uart_1::HW_UDMA_UART_1_BASE,
                        uart_buf_addr,
                        uart_buf_addr,
                    )
                };
                #[cfg(not(feature = "bao1x-evb"))]
                let mut udma_uart = unsafe {
                    // safety: this is safe to call, because we set up clock and events prior to calling new.
                    udma::Uart::get_handle(
                        utra::udma_uart_2::HW_UDMA_UART_2_BASE,
                        uart_buf_addr,
                        uart_buf_addr,
                    )
                };
                udma_uart.write(&buf);
            }
        }
    }
}

use core::fmt::{Error, Write};
impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> {
        let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
        #[cfg(feature = "bao1x-evb")]
        let mut udma_uart = unsafe {
            // safety: this is safe to call, because we set up clock and events prior to calling new.
            udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
        };
        #[cfg(not(feature = "bao1x-evb"))]
        let mut udma_uart = unsafe {
            // safety: this is safe to call, because we set up clock and events prior to calling new.
            udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
        };
        let mut c = 0u8;
        udma_uart.read_async(&mut c);

        let mut irqarray5 = CSR::new(utra::irqarray5::HW_IRQARRAY5_BASE as *mut u32);
        // read & clear the pending bits
        let pending = irqarray5.r(utra::irqarray5::EV_PENDING);
        // crate::println!("pending {:x} {}", pending, unsafe { char::from_u32_unchecked(c as u32) });
        irqarray5.wo(utra::irqarray5::EV_PENDING, pending);
        if pending != 0 { Some(c) } else { None }
    }
}

#[macro_use]
pub mod debug_print_hardware {
    #[macro_export]
    macro_rules! print
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!(crate::debug::Uart {}, $($args)+);
        });
    }
}

#[macro_export]
macro_rules! println
{
    () => ({
        $crate::print!("\r\n")
    });
    ($fmt:expr) => ({
        $crate::print!(concat!($fmt, "\r\n"))
    });
    ($fmt:expr, $($args:tt)+) => ({
        $crate::print!(concat!($fmt, "\r\n"), $($args)+)
    });
}

pub fn setup_rx(perclk: u32) -> bao1x_hal::udma::Uart {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let udma_global = GlobalConfig::new();
    #[cfg(feature = "bao1x-evb")]
    {
        iox.set_alternate_function(IoxPort::PD, 13, IoxFunction::AF1);
        iox.set_alternate_function(IoxPort::PD, 14, IoxFunction::AF1);
        // rx as input, with pull-up
        iox.set_gpio_dir(IoxPort::PD, 13, IoxDir::Input);
        iox.set_gpio_pullup(IoxPort::PD, 13, IoxEnable::Enable);
        // tx as output
        iox.set_gpio_dir(IoxPort::PD, 14, IoxDir::Output);

        udma_global.clock_on(PeriphId::Uart1);
        udma_global.map_event(
            PeriphId::Uart1,
            PeriphEventType::Uart(EventUartOffset::Rx),
            EventChannel::Channel0,
        );
        udma_global.map_event(
            PeriphId::Uart1,
            PeriphEventType::Uart(EventUartOffset::Tx),
            EventChannel::Channel1,
        );
    }
    #[cfg(not(feature = "bao1x-evb"))]
    {
        iox.set_alternate_function(IoxPort::PB, 13, IoxFunction::AF1);
        iox.set_alternate_function(IoxPort::PB, 14, IoxFunction::AF1);
        // rx as input, with pull-up
        iox.set_gpio_dir(IoxPort::PB, 13, IoxDir::Input);
        iox.set_gpio_pullup(IoxPort::PB, 13, IoxEnable::Enable);
        // tx as output
        iox.set_gpio_dir(IoxPort::PB, 14, IoxDir::Output);

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
    }

    let baudrate: u32 = crate::UART_BAUD;
    let freq: u32 = perclk / 2;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    #[cfg(feature = "bao1x-evb")]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    #[cfg(not(feature = "bao1x-evb"))]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    udma_uart.set_baud(baudrate, freq);
    udma_uart.setup_async_read();

    // setup interrupt here
    let mut uart_irq = UartIrq::new();
    #[cfg(feature = "bao1x-evb")]
    uart_irq.rx_irq_ena(udma::UartChannel::Uart1, true);
    #[cfg(not(feature = "bao1x-evb"))]
    uart_irq.rx_irq_ena(udma::UartChannel::Uart2, true);

    udma_uart
}

// ==== DUART-only debug print ==== -> this is used for USB feedback to avoid Tx loops on USB
/// Placeholder for debug
pub struct Duart {}

impl Duart {
    /// Print a character
    pub fn putc(&self, c: u8) {
        let base = utra::duart::HW_DUART_BASE as *mut u32;
        let mut uart = CSR::new(base);
        if uart.rf(utra::duart::SFR_CR_SFR_CR) == 0 {
            uart.wfo(utra::duart::SFR_CR_SFR_CR, 1);
        }
        while uart.r(utra::duart::SFR_SR) != 0 {}
        uart.wo(utra::duart::SFR_TXD, c as u32);
    }
}

impl Write for Duart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}

#[macro_use]
/// Hardware debug print module
pub mod debug_print_duart {
    #[macro_export]
    macro_rules! print_d
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!(crate::debug::Duart {}, $($args)+);
        });
    }
}

#[macro_export]
macro_rules! println_d
{
    () => ({
        $crate::print_d!("\r\n")
    });
    ($fmt:expr) => ({
        $crate::print_d!(concat!($fmt, "\r\n"))
    });
    ($fmt:expr, $($args:tt)+) => ({
        $crate::print_d!(concat!($fmt, "\r\n"), $($args)+)
    });
}
