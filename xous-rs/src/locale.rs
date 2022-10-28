#[cfg(feature="lang-ja")]
pub const LANG: &str = "ja";
#[cfg(feature="lang-zh")]
pub const LANG: &str = "zh";
#[cfg(feature="lang-en-tts")]
pub const LANG: &str = "en-tts";
#[cfg(feature="lang-fr")]
pub const LANG: &str = "fr";
#[cfg(not(any(
    feature="lang-ja",
    feature="lang-zh",
    feature="lang-en-tts",
    feature="lang-fr"
)))]
pub const LANG: &str = "en";
