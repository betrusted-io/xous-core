use enumset::EnumSet;
use rkyv::{Archive, Deserialize, Serialize};
use ux_api::minigfx::*;

use super::attach::Attach;
use crate::PostFlag;

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Post {
    author_id: u16,
    timestamp: u64,
    text: String,
    attach: Option<Attach>,
    pub flags: u16,
    pub bounding_box: Option<Rectangle>,
}

#[allow(dead_code)]
impl Post {
    pub fn new(author_id: u16, timestamp: u64, text: &str, attach: Option<Attach>) -> Self {
        Self { author_id, timestamp, text: text.to_string(), attach, flags: 0, bounding_box: None }
    }

    pub fn author_id(&self) -> u16 { self.author_id }

    pub fn flag_is(&self, flag: PostFlag) -> bool { self.flags_get().contains(flag) }

    pub fn flags_get(&self) -> EnumSet<PostFlag> { EnumSet::<PostFlag>::from_u16(self.flags) }

    pub fn flags_set(&mut self, flags: EnumSet<PostFlag>) { self.flags = flags.as_u16(); }

    pub fn text(&self) -> &str { self.text.as_str() }

    pub fn timestamp(&self) -> u64 { self.timestamp }
}
