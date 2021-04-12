pub(crate) const SERVER_NAME_TRNG: &str     = "_Callback test client 2_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get one or two 32-bit words of TRNG data
    GetTrng,
}
