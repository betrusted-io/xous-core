use super::fixtures::CardFixture;

pub struct CardState {
    pub applet_selected: bool,
    pub pw1_verified: bool,
    pub pw3_verified: bool,
    pub fixture: &'static CardFixture,
    pub response_buffer: Vec<u8>,
    pub response_offset: usize,
}

impl CardState {
    pub fn new(fixture: &'static CardFixture) -> Self {
        Self {
            applet_selected: false,
            pw1_verified: false,
            pw3_verified: false,
            fixture,
            response_buffer: Vec::new(),
            response_offset: 0,
        }
    }

    pub fn clear_chunk_state(&mut self) {
        self.response_buffer.clear();
        self.response_offset = 0;
    }
}
