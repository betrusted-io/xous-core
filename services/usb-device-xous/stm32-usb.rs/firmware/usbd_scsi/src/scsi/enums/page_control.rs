use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum PageControl {
    /// Current values
    CurrentValues = 0b00,
    /// Changeable values
    ChangeableValues = 0b01,
    /// Default values
    DefaultValues = 0b10,
    /// Saved values
    SavedValues = 0b11,
}

impl Default for PageControl {
    fn default() -> Self {
        PageControl::CurrentValues
    }
}
