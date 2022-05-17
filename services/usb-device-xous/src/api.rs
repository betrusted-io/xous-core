pub(crate) const SERVER_NAME_USB_DEVICE: &'static str = "_Xous USB device driver_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Returns the link status
    LinkStatus,
    /// Send a keyboard code
    SendKeyCode,
    /// Send a string
    SendString,
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

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UsbString {
    pub s: xous_ipc::String::<4000>,
    pub sent: Option<u32>,
}