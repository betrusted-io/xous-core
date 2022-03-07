pub(crate) const SERVER_NAME_TTS: &str     = "_Text to speech front end_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Basic, synchronous, non-abortable conversion of a string to audible speech
    TextToSpeech,
    /// Callback for the codec crate
    CodecCb,
    /// Exits the server
    Quit,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TtsFrontendMsg {
    pub text: xous_ipc::String::<2048>,
}