use enum_dispatch::enum_dispatch;

use super::*;

#[enum_dispatch(ActionApi)]
pub enum ActionType {
    #[cfg(feature = "textentry")]
    TextEntry(TextEntry),
    #[cfg(feature = "bip39entry")]
    Bip39Entry(Bip39Entry),
    #[cfg(feature = "radiobuttons")]
    RadioButtons(RadioButtons),
    #[cfg(feature = "checkboxes")]
    CheckBoxes(CheckBoxes),
    #[cfg(feature = "slider")]
    Slider(Slider),
    #[cfg(feature = "notification")]
    Notification(Notification),
    #[cfg(feature = "ditherpunk")]
    Image,
    #[cfg(feature = "consoleinput")]
    ConsoleInput,
}

#[enum_dispatch]
pub trait ActionApi {
    fn height(&self, glyph_height: isize, margin: isize, _modal: &Modal) -> isize {
        glyph_height + margin * 2
    }
    fn redraw(&self, _at_height: isize, _modal: &Modal) { unimplemented!() }
    fn close(&mut self) {}
    fn is_password(&self) -> bool { false }
    /// navigation is one of '∴' | '←' | '→' | '↑' | '↓'
    fn key_action(&mut self, _key: char) -> Option<ValidatorErr> { None }
    fn set_action_opcode(&mut self, _op: u32) {}
}
