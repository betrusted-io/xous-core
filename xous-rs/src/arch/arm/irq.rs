/// These are Xous-specific IRQ numbers that doesn't directly correspond to the chip's IRQ numbers.
/// In fact, there are 70+ interrupt sources in ATSAMA5D27 that won't fit in the 32 bit map used in Xous.
/// The current solution is to pick a subset of 32 interrupts that are most likely to be used by Xous and specify them here.
#[derive(Debug)]
pub enum IrqNumber {
    PeriodicIntervalTimer = 0,

    Uart0 = 1,
    Uart1 = 2,
    Uart2 = 3,
    Uart3 = 4,
    Uart4 = 5,

    Pioa  = 6,
    Piob  = 7,
    Pioc  = 8,
    Piod  = 9,

    Isi   = 10,
    Lcdc  = 11,

    Uhphs = 12,
    Udphs = 13,

    Tc0   = 14,
    Tc1   = 15,
}

impl TryFrom<usize> for IrqNumber {
    type Error = ();

    fn try_from(value: usize) -> Result<IrqNumber, Self::Error> {
        use IrqNumber::*;
        match value {
            0 => Ok(PeriodicIntervalTimer),

            1 => Ok(Uart0),
            2 => Ok(Uart1),
            3 => Ok(Uart2),
            4 => Ok(Uart3),
            5 => Ok(Uart4),

            6 => Ok(Pioa),
            7 => Ok(Piob),
            8 => Ok(Pioc),
            9 => Ok(Piod),

            10 => Ok(Isi),
            11 => Ok(Lcdc),

            12 => Ok(Uhphs),
            13 => Ok(Udphs),

            14 => Ok(Tc0),
            15 => Ok(Tc1),

            _ => Err(())
        }
    }
}
