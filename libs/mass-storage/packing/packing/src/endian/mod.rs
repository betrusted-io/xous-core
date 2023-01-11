use crate::Bit;

mod little;
pub use little::*;

mod big;
pub use big::*;

/// Trait that covers functionality required to deal with non aligned endian sensitive fields
/// in packed structs
///
/// For example, 10 bits LE offset by 2:
///
/// | byte | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |
/// |------|---|---|---|---|---|---|---|---|
/// | 0    | 0 | 0 | F | E | D | C | B | A |
/// | 1    | J | I | H | G | 0 | 0 | 0 | 0 |
///
/// Should become:
///
/// | byte | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |
/// |------|---|---|---|---|---|---|---|---|
/// | 0    | H | G | F | E | D | C | B | A |
/// | 1    | 0 | 0 | 0 | 0 | 0 | 0 | J | I |
///
pub trait Endian {
    const IS_LITTLE: bool;

    /// Align the bits in slice to usual 8 bit byte boundaries in the endianness represented by
    /// the implementing type
    ///
    /// Also masks away bytes outside the range specified by `S` and `E`.
    /// Simple memcopy if `S` == 7 and `E` == 0.
    ///
    /// `S` and `E` type parameters represent bit positions with 7 being the most significant bit and
    /// 0 being the least significant bit. `S` is the first included bit in the first byte of the slice.
    /// `E` is the last included bit in the last byte of the slice.
    fn align_field_bits<S: Bit, E: Bit>(input_bytes: &[u8], output_bytes: &mut [u8]);

    /// Take nice 8 bit aligned bytes and shift them to align with the bits specified by `S` and `E`
    /// in an endian aware way - this means the most significant bits will be masked away rather than
    /// the least significant bits. Does not perform any range checks to determine if data is being
    /// truncated.
    ///
    /// Data is ORed into the output byte slice so it is acceptable for there to be data already in
    /// the slice outside of the field defined by `S` and `E`. Bits within the field must be set to 0
    /// prior to calling this function.
    /// TODO: Clear out data inside the field so dirty buffers can be reused.
    ///
    /// `S` and `E` type parameters represent bit positions with 7 being the most significant bit and
    /// 0 being the least significant bit. `S` is the first included bit in the first byte of the slice.
    /// `E` is the last included bit in the last byte of the slice.
    fn restore_field_bits<S: Bit, E: Bit>(input_bytes: &[u8], output_bytes: &mut [u8]);
}
