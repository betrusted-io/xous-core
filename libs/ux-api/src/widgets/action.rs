use core::ops::Not;

use super::*;

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
