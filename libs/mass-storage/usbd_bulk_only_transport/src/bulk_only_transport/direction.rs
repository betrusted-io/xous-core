use packing::Packed;

/// The direction of a data transfer
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum Direction {
    /// Host to device, OUT in USB parlance
    HostToDevice = 0x00,
    /// Device to host, IN in USB parlance
    DeviceToHost = 0x80,
}