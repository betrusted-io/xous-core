use core::sync::atomic::{AtomicBool, Ordering::SeqCst};

use bao1x_api::*;
use bao1x_hal::iox::Iox;
#[cfg(feature = "unsafe-dev")]
use bao1x_hal::udma::UartIrq;
use bao1x_hal::{udma, udma::GlobalConfig};
use utralib::generated::*;

pub static USE_CONSOLE: AtomicBool = AtomicBool::new(false);

/// A trait for serial like drivers which allows reading from a source.
#[cfg(feature = "unsafe-dev")]
pub trait SerialRead {
    /// Read a single byte.
    fn getc(&mut self) -> Option<u8>;
}
pub struct Uart {}

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
            let buf: [u8; 1] = [c];
            let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
            let mut udma_uart = unsafe {
                // safety: this is safe to call, because we set up clock and events prior to calling new.
                udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
            };
            udma_uart.write(&buf);
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

#[cfg(feature = "unsafe-dev")]
impl SerialRead for Uart {
    fn getc(&mut self) -> Option<u8> {
        let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
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

#[cfg(feature = "unsafe-dev")]
pub fn setup_rx(perclk: u32) -> bao1x_hal::udma::Uart {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let udma_global = GlobalConfig::new();
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
    let freq: u32 = perclk;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    udma_uart.set_baud(baudrate, freq);
    udma_uart.setup_async_read();

    // setup interrupt here
    let mut uart_irq = UartIrq::new();
    uart_irq.rx_irq_ena(udma::UartChannel::Uart2, true);

    udma_uart
}

#[cfg(not(feature = "unsafe-dev"))]
pub fn setup_tx(perclk: u32) -> bao1x_hal::udma::Uart {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let udma_global = GlobalConfig::new();

    iox.set_alternate_function(IoxPort::PB, 14, IoxFunction::AF1);
    // tx as output
    iox.set_gpio_dir(IoxPort::PB, 14, IoxDir::Output);

    udma_global.clock_on(PeriphId::Uart2);
    udma_global.map_event(
        PeriphId::Uart2,
        PeriphEventType::Uart(EventUartOffset::Tx),
        EventChannel::Channel1,
    );

    let baudrate: u32 = crate::UART_BAUD;
    let freq: u32 = perclk;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    let udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    udma_uart.set_baud(baudrate, freq);

    udma_uart
}
