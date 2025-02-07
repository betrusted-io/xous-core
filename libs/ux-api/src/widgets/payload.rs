use zeroize::Zeroize;

use super::*;

/// We use a new type for item names, so that it's easy to resize this as needed.
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ItemName(String);
impl ItemName {
    pub fn new(name: &str) -> Self { ItemName(String::from(name)) }

    pub fn as_str(&self) -> &str { self.0.as_str() }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq, Default)]
pub struct Bip39EntryPayload {
    // up to 32 bytes (256 bits) could be entered
    pub data: [u8; 32],
    // the actual length entered is reported here
    pub len: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayload {
    dirty: bool,
    pub content: String,
    pub placeholder: Option<String>,
    pub placeholder_persist: bool,
    pub insertion_point: Option<usize>,
}

impl TextEntryPayload {
    pub fn new() -> Self {
        TextEntryPayload {
            dirty: Default::default(),
            content: Default::default(),
            placeholder: Default::default(),
            placeholder_persist: false,
            insertion_point: None,
        }
    }

    pub fn new_with_fields(content: String, placeholder: Option<String>) -> Self {
        TextEntryPayload {
            dirty: false,
            content,
            placeholder,
            placeholder_persist: false,
            insertion_point: None,
        }
    }

    /// Ensures that 0's are written to the storage of this struct, and not optimized out; important for
    /// password fields.
    pub fn volatile_clear(&mut self) { self.content.zeroize(); }

    pub fn as_str(&self) -> &str { self.content.as_str() }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SliderPayload(pub u32);
impl SliderPayload {
    pub fn new(value: u32) -> Self { SliderPayload(value) }
}

#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RadioButtonPayload(pub ItemName); // returns the name of the item corresponding to the radio button selection
impl RadioButtonPayload {
    pub fn new(name: &str) -> Self { RadioButtonPayload(ItemName::new(name)) }

    pub fn as_str(&self) -> &str { self.0.as_str() }

    pub fn clear(&mut self) { self.0.0.clear(); }
}
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CheckBoxPayload(pub [Option<ItemName>; MAX_ITEMS]); // returns a list of potential items that could be selected
impl CheckBoxPayload {
    pub fn new() -> Self { CheckBoxPayload([const { None }; MAX_ITEMS]) }

    pub fn payload(self) -> [Option<ItemName>; MAX_ITEMS] { self.0 }

    pub fn contains(&self, name: &str) -> bool {
        for maybe_item in self.0.iter() {
            if let Some(item) = maybe_item {
                if item.as_str() == name {
                    return true;
                }
            }
        }
        false
    }

    pub fn add(&mut self, name: &str) -> bool {
        if self.contains(name) {
            return true;
        }
        for maybe_item in self.0.iter_mut() {
            if maybe_item.is_none() {
                *maybe_item = Some(ItemName::new(name));
                return true;
            }
        }
        false
    }

    pub fn remove(&mut self, name: &str) -> bool {
        for maybe_item in self.0.iter_mut() {
            if let Some(item) = maybe_item {
                if item.as_str() == name {
                    *maybe_item = None;
                    return true;
                }
            }
        }
        false
    }
}
