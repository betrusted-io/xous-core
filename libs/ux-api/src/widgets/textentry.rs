use core::cell::Cell;
use core::fmt::Write;
use std::cell::RefCell;

use super::*;

const MAX_FIELDS: i16 = 10;
pub const MAX_ITEMS: usize = 8;

pub type ValidatorErr = String;
pub type Payloads = [TextEntryPayload; MAX_FIELDS as usize];

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayloads(Payloads, usize);

impl TextEntryPayloads {
    pub fn first(&self) -> TextEntryPayload { self.0[0].clone() }

    pub fn content(&self) -> Vec<TextEntryPayload> { self.0[..self.1].to_vec() }
}

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible = 0,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars = 1,
    /// all chars hidden as *
    Hidden = 2,
}

#[derive(Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    // validator borrows the text entry payload, and returns an error message if something didn't go well.
    // validator takes as ragument the current action_payload, and the current action_opcode
    pub validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    pub action_payloads: Vec<TextEntryPayload>,

    max_field_amount: u32,
    selected_field: i16,
    field_height: Cell<i16>,
    /// track if keys were hit since initialized: this allows us to clear the default text,
    /// instead of having it re-appear every time the text area is cleared
    keys_hit: [bool; MAX_FIELDS as usize],
    // gam: crate::Gam, // no GAM field because this needs to be a clone-capable structure. We create a GAM
    // handle when we need it.
    /// Stores the allowed height of a given text line, based on the contents and the space available
    /// in the box. The height of a given line may be limited to make sure there is enough space for
    /// later lines to be rendered.
    action_payloads_allowed_heights: RefCell<Vec<i16>>,
}

impl Default for TextEntry {
    fn default() -> Self {
        Self {
            is_password: Default::default(),
            visibility: TextEntryVisibility::Visible,
            action_conn: Default::default(),
            action_opcode: Default::default(),
            validator: Default::default(),
            selected_field: Default::default(),
            action_payloads: Default::default(),
            max_field_amount: 0,
            field_height: Cell::new(0),
            keys_hit: [false; MAX_FIELDS as usize],
            action_payloads_allowed_heights: RefCell::new(Vec::new()),
        }
    }
}

impl TextEntry {
    pub fn new(
        is_password: bool,
        visibility: TextEntryVisibility,
        action_conn: xous::CID,
        action_opcode: u32,
        action_payloads: Vec<TextEntryPayload>,
        validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    ) -> Self {
        if action_payloads.len() as i16 > MAX_FIELDS {
            panic!("can't have more than {} fields, found {}", MAX_FIELDS, action_payloads.len());
        }

        Self {
            is_password,
            visibility,
            action_conn,
            action_opcode,
            action_payloads,
            validator,
            ..Default::default()
        }
    }

    pub fn reset_action_payloads(&mut self, fields: u32, placeholders: Option<[Option<(String, bool)>; 10]>) {
        let mut payload = vec![TextEntryPayload::default(); fields as usize];

        if let Some(placeholders) = placeholders {
            for (index, element) in payload.iter_mut().enumerate() {
                if let Some((p, persist)) = &placeholders[index] {
                    element.placeholder = Some(p.to_string());
                    element.placeholder_persist = *persist;
                } else {
                    element.placeholder = None;
                    element.placeholder_persist = false;
                }
                element.insertion_point = None;
            }
        }

        self.action_payloads = payload;
        self.max_field_amount = fields;
        self.keys_hit = [false; MAX_FIELDS as usize];
    }

    fn get_bullet_margin(&self) -> i16 {
        if self.action_payloads.len() > 1 {
            17 // this is the margin for drawing the selection bullet
        } else {
            0 // no selection bullet
        }
    }
}

use crate::widgets::ActionApi;
impl ActionApi for TextEntry {}
