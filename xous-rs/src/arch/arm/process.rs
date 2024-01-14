#[derive(Debug, PartialEq, Eq)]
pub struct ProcessArgs;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInit;
impl TryFrom<[usize; 7]> for ProcessInit {
    type Error = crate::Error;

    fn try_from(_src: [usize; 7]) -> Result<ProcessInit, Self::Error> { todo!() }
}
impl Into<[usize; 7]> for &ProcessInit {
    fn into(self) -> [usize; 7] { todo!() }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProcessStartup;
impl From<&[usize; 7]> for ProcessStartup {
    fn from(_src: &[usize; 7]) -> ProcessStartup { todo!() }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(_src: [usize; 8]) -> ProcessStartup { todo!() }
}

impl Into<[usize; 7]> for &ProcessStartup {
    fn into(self) -> [usize; 7] { todo!() }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProcessKey;
