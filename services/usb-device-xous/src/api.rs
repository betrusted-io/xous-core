pub(crate) const SERVER_NAME_USB_DEVICE: &'static str = "_Xous USB device driver_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Returns the link status
    LinkStatus,
    /// Send a keyboard code
    SendKeyCode,
    /// Get the current LED state
    GetLedState,
    /// Switch to a specified device core
    SwitchCores,
    /// Check which core is connected
    WhichCore,
    /// Restrict the debug core
    RestrictDebugAccess,
    /// Retrieve restriction state
    IsRestricted,
    /// Set-and-check of USB debug restriction
    DebugUsbOp,

    /// Handle the USB interrupt
    UsbIrqHandler,
    /// Suspend/resume callback
    SuspendResume,
    /// Exits the server
    Quit,
}
