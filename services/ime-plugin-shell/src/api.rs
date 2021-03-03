#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub struct Prediction {
    pub index: u32,
    pub string: xous::String<4096>,
}

#[allow(dead_code)]
#[derive(Debug, rkyv::Archive, rkyv::Unarchive)]
pub enum Opcode {
    /// update with the latest input candidate. Replaces the previous input.
    Input(xous::String<4096>),

    /// feed back to the IME plugin as to what was picked, so predictions can be updated
    Picked(xous::String<4096>),

    /// fetch the prediction at a given index, where the index is ordered from 0..N, where 0 is the most likely prediction
    /// if there is no prediction available, just return an empty string
    Prediction(Prediction),
}
