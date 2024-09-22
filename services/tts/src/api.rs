pub(crate) const SERVER_NAME_TTS: &str = "_Text to speech front end_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Basic, interruptable conversion of a string to audible speech
    TextToSpeech,
    /// Non-interruptable conversion of a string to audible speech. Blocks until the phrase is finished.
    TextToSpeechBlocking,
    /// Stops audio playback immediately. Does not stop wave generation.
    CodecStop,
    /// Set words per minute
    SetWordsPerMinute,
    /// Exits the server
    Quit,
}

#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TtsFrontendMsg {
    pub text: String,
}
