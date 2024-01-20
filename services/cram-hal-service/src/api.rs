#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    /// Allocate an IFRAM block
    MapIfram,
    /// Deallocate an IFRAM block
    UnmapIfram,

    /// Gutter for Invalid Calls
    InvalidCall,

    /// Exit server
    Quit,
}
