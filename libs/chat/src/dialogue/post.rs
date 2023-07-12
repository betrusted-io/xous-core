use super::attach::Attach;

use crate::PostFlag;
use enumset::EnumSet;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Post {
    author_id: u16,
    timestamp: u32,
    text: String,
    attach: Option<Attach>,
    pub flags: u16,
}

#[allow(dead_code)]
impl Post {
    pub fn new(author_id: u16, timestamp: u32, text: &str, attach: Option<Attach>) -> Self {
        Self {
            author_id: author_id,
            timestamp: timestamp,
            text: text.to_string(),
            attach: attach,
            flags: 0,
        }
    }

    pub fn author_id(&self) -> u16 {
        self.author_id
    }

    pub fn flag_is(&self, flag: PostFlag) -> bool {
        self.flags_get().contains(flag)
    }

    pub fn flags_get(&self) -> EnumSet<PostFlag> {
        EnumSet::<PostFlag>::from_u16(self.flags)
    }

    pub fn flags_set(&mut self, flags: EnumSet<PostFlag>) {
        self.flags = flags.as_u16();
    }

    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }
}
