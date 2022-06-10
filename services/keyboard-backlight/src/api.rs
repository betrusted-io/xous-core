pub const KBB_SERVER_NAME: &str = "_Keyboard backlight_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum KbbOps {
    Keypress,
    TurnLightsOff,
    TurnLightsOn,
    EnableAutomaticBacklight,
    DisableAutomaticBacklight,
    Status,
}