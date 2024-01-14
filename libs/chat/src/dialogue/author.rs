use enumset;
use enumset::EnumSet;
use rkyv::{Archive, Deserialize, Serialize};

use crate::api::AuthorFlag;

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Author {
    pub name: String,
    pub icon: Option<char>,
    pub flags: u16,
}

#[allow(dead_code)]
impl Author {
    pub fn new(name: &str) -> Self { Self { name: name.to_string(), icon: None, flags: 0 } }

    pub fn flag_is(&self, flag: AuthorFlag) -> bool { self.flags_get().contains(flag) }

    pub fn flags_get(&self) -> EnumSet<AuthorFlag> { EnumSet::<AuthorFlag>::from_u16(self.flags) }

    pub fn flags_set(&mut self, flags: EnumSet<AuthorFlag>) { self.flags = flags.as_u16(); }
}
