pub mod keyboard;

// TRNG server is only needed for baosec; dabao can cheat and use the kernel TRNG port
#[cfg(feature = "board-baosec")]
mod baosec_hw;
pub mod bio;
pub mod rtc;
pub mod susres;
pub mod trng;
