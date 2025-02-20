#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
    CamIrq,
    InvalidCall,
}

pub const SERVER_NAME_GFX: &str = "_Graphics_";
