use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Serialize, Deserialize, Debug)]
pub enum Attach {
    Png(),
    Jpg(),
}

impl Attach {}
