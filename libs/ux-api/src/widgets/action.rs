use enum_dispatch::enum_dispatch;

use super::*;

#[enum_dispatch(ActionApi)]
pub enum ActionType {
    #[cfg(feature = "textentry")]
    TextEntry,
    #[cfg(feature = "bip39entry")]
    Bip39Entry,
    #[cfg(feature = "radiobuttons")]
    RadioButtons,
    #[cfg(feature = "checkboxes")]
    CheckBoxes,
    #[cfg(feature = "slider")]
    Slider,
    #[cfg(feature = "notification")]
    Notification,
    #[cfg(feature = "ditherpunk")]
    Image,
    #[cfg(feature = "consoleinput")]
    ConsoleInput,
}

/* TODO: turn Modal into a trait */
#[enum_dispatch]
pub trait ActionApi<Modal> {
    /// Returns the height of the widget for layout use
    fn height(&self, glyph_height: i16, margin: i16, _modal: &Modal) -> i16 { glyph_height + margin * 2 }
    /// Triggers a redraw of the action object
    fn redraw(&self, _at_height: i16, _modal: &Modal) { unimplemented!() }
    fn close(&mut self) {}
    fn is_password(&self) -> bool { false }
    /// navigation is one of '∴' | '←' | '→' | '↑' | '↓'
    fn key_action(&mut self, _key: char) -> Option<ValidatorErr> { None }
    fn set_action_opcode(&mut self, _op: u32) {}
}
