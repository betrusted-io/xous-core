use utralib::*;

/*
The RTC is a weird bit of IP. It's a proprietary ARM block called the "PL031".
Because we don't have the source code for it, we couldn't extract a standard UTRA
header for the block. However, apparently ~ChatGPT~ knows about it, so I was able
to ask it to help generate the following user guide.

Query: "give me a C header file for the PL031 RTC block".

Here is a concise C header for the ARM PrimeCell PL031 RTC block. Field offsets follow the ARM TRM. Adjust types or volatile rules as needed for your environment.

#ifndef PL031_H
#define PL031_H

#include <stdint.h>

#define PL031_BASE <insert location>

/* Register offsets */
#define PL031_DR        0x00    /* Data register */
#define PL031_MR        0x04    /* Match register */
#define PL031_LR        0x08    /* Load register */
#define PL031_CR        0x0C    /* Control register */
#define PL031_IMSC      0x10    /* Interrupt mask set/clear */
#define PL031_RIS       0x14    /* Raw interrupt status */
#define PL031_MIS       0x18    /* Masked interrupt status */
#define PL031_ICR       0x1C    /* Interrupt clear */
#define PL031_PERIPH_ID 0xFE0
#define PL031_PCELL_ID  0xFF0

/* Bit definitions */
#define PL031_CR_EN     (1U << 0)
#define PL031_IMSC_MIS  (1U << 0)
#define PL031_RIS_RIS   (1U << 0)
#define PL031_MIS_MIS   (1U << 0)
#define PL031_ICR_CLR   (1U << 0)

/* Access helpers */
static inline void pl031_write(uint32_t offset, uint32_t val) {
    *(volatile uint32_t *)(PL031_BASE + offset) = val;
}

static inline uint32_t pl031_read(uint32_t offset) {
    return *(volatile uint32_t *)(PL031_BASE + offset);
}

#endif

LR (Load register, 0x08)
Used to set the starting epoch. On the next second boundary, LR is copied into DR and becomes the running counter.

MR (Match register, 0x04)
Stores a compare value. When DR equals MR, the block asserts its interrupt. Software typically uses this for scheduled wakeups or periodic tasks.

CR (Control register, 0x0C)
Bit 0 enables or disables the RTC. When disabled, the counter stops incrementing.

IMSC (Interrupt mask set or clear, 0x10)
Controls whether the match event generates an interrupt to the CPU. Bit 0 is the mask.

RIS (Raw interrupt status, 0x14)
Reflects the unmasked interrupt state. Set when DR equals MR.

MIS (Masked interrupt status, 0x18)
Reflects the final interrupt state after masking. This is what software normally checks.

ICR (Interrupt clear, 0x1C)
Write 1 to bit 0 to clear the match interrupt so the next event can fire.

Query: "how would i actually use this to keep time"

To use the PL031 to keep time, treat it as a free running 1 Hz counter. The hardware increments the DR register once per second. The workflow has three steps.

1. Initialize the counter
Write an initial epoch value to LR, then enable the block.

/* Set initial epoch time, for example UNIX time */
pl031_write(PL031_LR, initial_epoch_seconds);

/* Enable RTC */
pl031_write(PL031_CR, PL031_CR_EN);

The PL031 copies LR into DR on the next tick, then DR increments once every second.

2. Read the current time
Just read DR whenever you need seconds since the epoch you chose.

uint32_t now = pl031_read(PL031_DR);

If you want a fuller timestamp, convert now to a struct tm equivalent in software.

3. Generate periodic events
If your system needs periodic wakeups or scheduling:

uint32_t t = pl031_read(PL031_DR);
pl031_write(PL031_MR, t + interval_seconds);
pl031_write(PL031_IMSC, PL031_IMSC_MIS);  /* unmask interrupt */

On interrupt, clear it and program the next match.

pl031_write(PL031_ICR, PL031_ICR_CLR);

Considerations

    If your system sleeps, the PL031 keeps running as long as its clock domain stays powered.

    If resets occur, reinitialize LR and CR.

    If subsecond precision is required, this peripheral cannot supply it; layer a software counter from a higher frequency timer.


~Good enough for me!~

*/

// data register
pub const DR: Register = Register::new(0, 0xFFFF_FFFF);
// match register
pub const MR: Register = Register::new(1, 0xFFFF_FFFF);
// load register
pub const LR: Register = Register::new(2, 0xFFFF_FFFF);

// control register
pub const CR: Register = Register::new(3, 1);
pub const CR_EN: Field = Field::new(1, 0, CR);
// interrupt mask set/clear
pub const IMSC: Register = Register::new(4, 1);
pub const IMSC_MASK: Field = Field::new(1, 0, IMSC);
// raw interrupt status
pub const RIS: Register = Register::new(5, 1);
pub const RIS_STATUS: Field = Field::new(1, 0, RIS);
// masked interrupt status
pub const MIS: Register = Register::new(6, 1);
pub const MIS_STATUS: Field = Field::new(1, 0, MIS);
// interrupt clear
pub const ICR: Register = Register::new(7, 1);
pub const ICR_CLEAR: Field = Field::new(1, 0, ICR);

// IP ID registers
pub const PERI_ID: Register = Register::new(1016, 0xffff_ffff);
pub const PRIME_ID: Register = Register::new(1020, 0xffff_ffff);

pub const HW_RTC_BASE: usize = 0x40061000;

pub const RTC_NUMREGS: usize = 10;
