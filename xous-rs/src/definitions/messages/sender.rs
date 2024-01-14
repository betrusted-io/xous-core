use crate::PID;

#[derive(Debug, PartialEq, Eq, Ord, PartialOrd, Copy, Clone, Default)]
pub struct Sender {
    data: usize,
}

impl Sender {
    pub fn to_usize(&self) -> usize { self.data }

    pub fn from_usize(data: usize) -> Self { Sender { data } }

    pub fn pid(&self) -> Option<PID> {
        let pid_u8 = ((self.data >> 24) & 0xff) as u8;
        PID::new(pid_u8)
    }
}

impl core::fmt::Display for Sender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MessageSender.data: 0x{:08x}", self.data)
    }
}
