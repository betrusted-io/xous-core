pub(crate) const SERVER_NAME_USB_DEVICE: &'static str = "_Xous USB device driver_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Returns the link status
    LinkStatus,
    /// Send a keyboard code
    SendKeyCode,
    /// Get the current LED state
    GetLedState,

    /// Handle the USB interrupt
    UsbIrqHandler,
    /// Suspend/resume callback
    SuspendResume,
    /// Exits the server
    Quit,
}
