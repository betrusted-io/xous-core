use utralib::*;

use crate::ifram::IframRange;
use crate::udma::*;

// ----------------------------------- UART ------------------------------------
#[repr(usize)]
enum UartReg {
    Status = 0,
    Setup = 1,
}
impl Into<usize> for UartReg {
    fn into(self) -> usize { self as usize }
}

#[repr(usize)]
pub enum UartChannel {
    Uart0 = 0,
    Uart1 = 1,
    Uart2 = 2,
    Uart3 = 3,
}
impl Into<usize> for UartChannel {
    fn into(self) -> usize { self as usize }
}
impl TryFrom<PeriphId> for UartChannel {
    type Error = xous::Error;

    fn try_from(value: PeriphId) -> Result<Self, Self::Error> {
        match value {
            PeriphId::Uart0 => Ok(UartChannel::Uart0),
            PeriphId::Uart1 => Ok(UartChannel::Uart1),
            PeriphId::Uart2 => Ok(UartChannel::Uart2),
            PeriphId::Uart3 => Ok(UartChannel::Uart3),
            _ => Err(xous::Error::InvalidString),
        }
    }
}
/// UDMA UART wrapper. Contains all the warts on top of the Channel abstraction.
pub struct Uart {
    /// This is assumed to point to the base of the peripheral's UDMA register set.
    csr: CSR<u32>,
    #[allow(dead_code)] // suppress warning with `std` is not selected
    ifram: IframRange,
}

/// Blanket implementations to access the CSR within UART. Needed because you can't
/// have default fields in traits: https://github.com/rust-lang/rfcs/pull/1546
impl Udma for Uart {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}
/// The sum of UART_TX_BUF_SIZE + UART_RX_BUF_SIZE should be 4096.
const UART_TX_BUF_SIZE: usize = 2048;
const UART_RX_BUF_START: usize = UART_TX_BUF_SIZE;
const UART_RX_BUF_SIZE: usize = 2048;
const RX_BUF_DEPTH: usize = 1;
impl Uart {
    /// Configures for N81
    ///
    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    ///
    /// Allocates a 4096-deep buffer for tx/rx purposes: the first 2048 bytes
    /// are used for Tx, the second 2048 bytes for Rx. If this buffer size has
    /// to change, be sure to update the loader, as it takes this as an assumption
    /// since no IFRAM allocator is running at that time.
    #[cfg(feature = "std")]
    pub unsafe fn new(channel: UartChannel, baud: u32, clk_freq: u32) -> Self {
        assert!(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE == 4096, "Configuration error in UDMA UART");
        let bank_addr = match channel {
            UartChannel::Uart0 => utra::udma_uart_0::HW_UDMA_UART_0_BASE,
            UartChannel::Uart1 => utra::udma_uart_1::HW_UDMA_UART_1_BASE,
            UartChannel::Uart2 => utra::udma_uart_2::HW_UDMA_UART_2_BASE,
            UartChannel::Uart3 => utra::udma_uart_3::HW_UDMA_UART_3_BASE,
        };
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(bank_addr),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map serial port");

        // now setup the channel
        let csr = CSR::new(uart.as_mut_ptr() as *mut u32);

        let clk_counter: u32 = (clk_freq + baud / 2) / baud;
        // setup baud, bits, parity, etc.
        csr.base()
            .add(Bank::Custom.into())
            .add(UartReg::Setup.into())
            .write_volatile(0x0306 | (clk_counter << 16));

        Uart { csr, ifram: IframRange::request(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE, None).unwrap() }
    }

    /// Gets a handle to the UART. Used for re-acquiring previously initialized
    /// UART hardware, such as from the loader booting into Xous
    ///
    /// Safety: only safe to call in the context of a previously initialized UART
    pub unsafe fn get_handle(csr_virt_addr: usize, udma_phys_addr: usize, udma_virt_addr: usize) -> Self {
        assert!(UART_RX_BUF_SIZE + UART_TX_BUF_SIZE == 4096, "Configuration error in UDMA UART");
        let csr = CSR::new(csr_virt_addr as *mut u32);
        Uart {
            csr,
            ifram: IframRange::from_raw_parts(
                udma_phys_addr,
                udma_virt_addr,
                UART_RX_BUF_SIZE + UART_TX_BUF_SIZE,
            ),
        }
    }

    pub fn set_baud(&self, baud: u32, clk_freq: u32) {
        let clk_counter: u32 = (clk_freq + baud / 2) / baud;
        // must disable the UART before adjusting any values
        unsafe {
            self.csr.base().add(Bank::Custom.into()).add(UartReg::Setup.into()).write_volatile(0x0);
        }
        // setup baud, bits, parity, etc.
        // safety: this is safe to call as long as the base address points at a valid UART.
        unsafe {
            self.csr
                .base()
                .add(Bank::Custom.into())
                .add(UartReg::Setup.into())
                .write_volatile(0x0306 | (clk_counter << 16));
        }
    }

    pub fn disable(&mut self) {
        self.wait_tx_done();
        // safe only in the context of a UART UDMA address
        unsafe {
            self.csr.base().add(Bank::Custom.into()).add(UartReg::Setup.into()).write_volatile(0x0050_0006);
        }
    }

    pub fn tx_busy(&self) -> bool {
        // safe only in the context of a UART UDMA address
        unsafe {
            (self.csr.base().add(Bank::Custom.into()).add(UartReg::Status.into()).read_volatile() & 1) != 0
        }
    }

    pub fn rx_busy(&self) -> bool {
        // safe only in the context of a UART UDMA address
        unsafe {
            (self.csr.base().add(Bank::Custom.into()).add(UartReg::Status.into()).read_volatile() & 2) != 0
        }
    }

    pub fn wait_tx_done(&self) {
        while self.udma_busy(Bank::Tx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        while self.tx_busy() {}
    }

    pub fn wait_rx_done(&self) {
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
    }

    /// `buf` is assumed to be a virtual address (in `std`), or a machine address
    /// (in baremetal mode). This function is safe because it will operate as intended
    /// within a given environment, so long as the `std` flag is applied correctly.
    ///
    /// When not in `std`, it's *also* assumed that `buf` is range-checked to be valid
    /// for the UDMA engine.
    ///
    /// returns: total length of bytes written
    pub fn write(&mut self, buf: &[u8]) -> usize {
        let mut writelen = 0;
        for chunk in buf.chunks(UART_TX_BUF_SIZE) {
            #[cfg(feature = "std")]
            {
                self.ifram.as_slice_mut()[..chunk.len()].copy_from_slice(chunk);
                // safety: the slice is in the physical range for the UDMA, and length-checked
                unsafe {
                    self.udma_enqueue(
                        Bank::Tx,
                        &self.ifram.as_phys_slice::<u8>()[..chunk.len()],
                        CFG_EN | CFG_SIZE_8,
                    );
                }
                writelen += chunk.len();
            }
            #[cfg(not(feature = "std"))]
            {
                self.ifram.as_slice_mut()[..chunk.len()].copy_from_slice(chunk);
                unsafe {
                    self.udma_enqueue(
                        Bank::Tx,
                        &self.ifram.as_phys_slice::<u8>()[..chunk.len()],
                        CFG_EN | CFG_SIZE_8,
                    );
                    writelen += chunk.len();
                }
            }

            self.wait_tx_done();
        }
        writelen
    }

    pub fn read(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(UART_RX_BUF_SIZE) {
            #[cfg(feature = "std")]
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + chunk.len()],
                    CFG_EN | CFG_SIZE_8,
                );
            }
            #[cfg(not(feature = "std"))]
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + chunk.len()],
                    CFG_EN | CFG_SIZE_8,
                );
            }
            self.wait_rx_done();
            #[cfg(feature = "std")]
            chunk.copy_from_slice(
                &self.ifram.as_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + chunk.len()],
            );
            #[cfg(not(feature = "std"))]
            unsafe {
                chunk.copy_from_slice(
                    &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + chunk.len()],
                );
            }
        }
    }

    /// Call this to read one character on receiving an interrupt.
    ///
    /// Note that if the interrupt is not handled fast enough, characters are simply dropped.
    ///
    /// Returns actual number of bytes read (0 or 1).
    pub fn read_async(&mut self, c: &mut u8) -> usize {
        let bank_addr = unsafe { self.csr().base().add(Bank::Rx as usize) };
        // retrieve total bytes available
        let pending = unsafe { bank_addr.add(DmaReg::Size.into()).read_volatile() } as usize;

        // recover the pending byte. Hard-coded for case of RX_BUF_DEPTH == 1
        assert!(RX_BUF_DEPTH == 1, "Need to refactor buf recovery code if RX_BUF_DEPTH > 1");
        #[cfg(feature = "std")]
        {
            *c = self.ifram.as_slice::<u8>()[UART_RX_BUF_START];
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            *c = self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START];
        }

        // queue the next round
        #[cfg(feature = "std")]
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + RX_BUF_DEPTH],
                CFG_EN | CFG_CONT,
            );
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + RX_BUF_DEPTH],
                CFG_EN | CFG_CONT,
            );
        }

        pending
    }

    /// Call this to prime the system for async reads. This must be called at least once if any characters
    /// are ever to be received.
    pub fn setup_async_read(&mut self) {
        #[cfg(feature = "std")]
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + RX_BUF_DEPTH],
                CFG_EN | CFG_CONT,
            );
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.ifram.as_phys_slice::<u8>()[UART_RX_BUF_START..UART_RX_BUF_START + RX_BUF_DEPTH],
                CFG_EN | CFG_CONT,
            );
        }
    }
}

#[derive(Debug)]
pub struct UartIrq {
    pub csr: CSR<u32>,
    #[cfg(feature = "std")]
    pub handlers: [Option<HandlerFn>; 4],
    #[cfg(feature = "std")]
    /// We can't claim the interrupt when the object is created, because the version we allocate
    /// inside `new()` is a temporary instance that exists on the stack. It's recommend that the
    /// caller put `UartIrq` inside a `Box` so that the location of the structure does not move
    /// around. Later on, when `register_handler()` is invoked, the address of `self` is used to
    /// pass into the handler. It is important that the caller ensures that `self` does not move around.
    interrupt_claimed: bool,
}
impl UartIrq {
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_IRQARRAY5_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map uart IRQ control");
        Self {
            csr: CSR::new(uart.as_ptr() as *mut u32),
            handlers: [None, None, None, None],
            interrupt_claimed: false,
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn new() -> Self {
        use riscv::register::vexriscv::mim;
        mim::write(mim::read() | (1 << utra::irqarray5::IRQARRAY5_IRQ));
        Self { csr: CSR::new(HW_IRQARRAY5_BASE as *mut u32) }
    }

    pub fn rx_irq_ena(&mut self, channel: UartChannel, enable: bool) {
        let val = if enable { 1 } else { 0 };
        match channel {
            UartChannel::Uart0 => self.csr.rmwf(utra::irqarray5::EV_ENABLE_UART0_RX, val),
            UartChannel::Uart1 => self.csr.rmwf(utra::irqarray5::EV_ENABLE_UART1_RX, val),
            UartChannel::Uart2 => self.csr.rmwf(utra::irqarray5::EV_ENABLE_UART2_RX, val),
            UartChannel::Uart3 => self.csr.rmwf(utra::irqarray5::EV_ENABLE_UART3_RX, val),
        }
    }

    #[cfg(feature = "std")]
    /// This needs to be invoked from a Pin'd Box wrapper of the UartIrq structure. Here is how the
    /// pattern looks:
    ///
    /// ```rust
    /// let mut uart_irq = Box::pin(bao1x_hal::udma::UartIrq::new());
    /// Pin::as_mut(&mut uart_irq).register_handler(udma::UartChannel::Uart1, uart_handler);
    /// ```
    ///
    /// What this does is bind a `UartIrq` instance to an address in the heap (via Box), and
    /// marks that address as non-moveable (via Pin), ensuring that the `register_handler` call's
    /// view of `self` stays around forever.
    ///
    /// Note: this does not also enable the interrupt channel, it just registers the handler
    ///
    /// Safety: the function is only safe to use if `self` has a `static` lifetime, that is, the
    /// `UartIrq` object will live the entire duration of the OS. If the object is destroyed,
    /// the IRQ handler will point to an invalid location and the system will crash. In general,
    /// we don't intend this kind of behavior, so we don't implement a `Drop` because simply
    /// de-allocating the interrupt handler on an accidental Drop is probably not intentional
    /// and can lead to even more confusing/harder-to-debug faults, i.e., the system won't crash,
    /// but it will simply stop responding to interrupts. As a philosophical point, if an unregister behavior
    /// is desired, it should be explicit.
    pub unsafe fn register_handler(
        mut self: std::pin::Pin<&mut Self>,
        channel: UartChannel,
        handler: HandlerFn,
    ) {
        if !self.interrupt_claimed {
            xous::claim_interrupt(
                utra::irqarray5::IRQARRAY5_IRQ,
                main_uart_handler,
                self.as_ref().get_ref() as *const UartIrq as *mut usize,
            )
            .expect("couldn't claim UART IRQ channel");
            self.interrupt_claimed = true;
        }

        self.handlers[channel as usize] = Some(handler);
    }
}

pub type HandlerFn = fn(usize, *mut usize);

#[cfg(feature = "std")]
fn main_uart_handler(irq_no: usize, arg: *mut usize) {
    // check ev_pending and dispatch handlers based on that
    let uartirq = unsafe { &mut *(arg as *mut UartIrq) };
    let pending = uartirq.csr.r(utra::irqarray5::EV_PENDING);
    if pending & uartirq.csr.ms(utra::irqarray5::EV_PENDING_UART0_RX, 1) != 0 {
        if let Some(h) = uartirq.handlers[0] {
            h(irq_no, arg);
        }
    }
    if pending & uartirq.csr.ms(utra::irqarray5::EV_PENDING_UART1_RX, 1) != 0 {
        if let Some(h) = uartirq.handlers[1] {
            h(irq_no, arg);
        }
    }
    if pending & uartirq.csr.ms(utra::irqarray5::EV_PENDING_UART2_RX, 1) != 0 {
        if let Some(h) = uartirq.handlers[2] {
            h(irq_no, arg);
        }
    }
    if pending & uartirq.csr.ms(utra::irqarray5::EV_PENDING_UART3_RX, 1) != 0 {
        if let Some(h) = uartirq.handlers[3] {
            h(irq_no, arg);
        }
    }
    // note that this will also clear other spurious interrupts without issuing a warning.
    uartirq.csr.wo(utra::irqarray5::EV_PENDING, pending);
}
