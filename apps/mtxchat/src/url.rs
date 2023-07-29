//! URL encoding and decoding

/// Encoding to use for conversions
pub const RFC3986: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~');

/// URL encode string
pub fn encode(v: &str) -> String {
    percent_encoding::utf8_percent_encode(v, RFC3986).to_string()
}

/// URL decode string
#[allow(dead_code)]
pub fn decode(v: &str) -> String {
    let decoded = percent_encoding::percent_decode_str(v)
        .decode_utf8()
        .expect("unable to decode");
    decoded.to_string()
}
