use super::*;

// This structure needs to be "shallow copy capable" so we can use it with
// the enum_actions API to update the progress state in an efficient manner.
// Thus it does not include its own GAM reference; instead we create one on
// the fly when needed.
#[derive(Debug, Copy, Clone)]
pub struct Slider {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: u32,
    pub is_progressbar: bool,
    pub is_password: bool,
    pub show_legend: bool,
    pub units: [u8; 8],
    pub units_len: usize,
}
impl Slider {
    pub fn new(
        action_conn: xous::CID,
        action_opcode: u32,
        min: u32,
        max: u32,
        step: u32,
        units: Option<&str>,
        initial_setting: u32,
        is_progressbar: bool,
        show_legend: bool,
    ) -> Self {
        let mut units_storage = [0u8; 8];
        let mut units_len = 0;
        if let Some(unit_str) = units {
            if unit_str.as_bytes().len() <= 8 {
                units_storage[..unit_str.as_bytes().len()].copy_from_slice(unit_str.as_bytes());
                units_len = unit_str.as_bytes().len();
            } else {
                log::error!(
                    "Unit string must be less than 8 *bytes* long (are you using unicode?), ignoring length {} string",
                    unit_str.as_bytes().len()
                );
            }
        }

        Slider {
            action_conn,
            action_opcode,
            is_password: false,
            is_progressbar,
            min,
            max,
            step,
            action_payload: initial_setting,
            units: units_storage,
            units_len,
            show_legend,
        }
    }

    pub fn set_is_progressbar(&mut self, setting: bool) { self.is_progressbar = setting; }

    pub fn set_is_password(&mut self, setting: bool) {
        // this will cause text to be inverted. Untrusted entities can try to set this,
        // but the GAM should defeat this for dialog boxes outside of the trusted boot
        // set because they can't achieve a high enough trust level.
        self.is_password = setting;
    }

    pub fn set_state(&mut self, state: u32) {
        if state < self.min {
            self.action_payload = self.min;
        } else if state > self.max {
            self.action_payload = self.max;
        } else {
            self.action_payload = state;
        }
    }
}

use crate::widgets::ActionApi;
impl ActionApi for Slider {}
