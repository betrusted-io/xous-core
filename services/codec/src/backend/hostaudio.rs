pub struct Codec {
}

impl Codec {
    pub fn new() -> Codec {
        Codec {
        }
    }
    pub fn suspend(&self) {
    }
    pub fn resume(&self) {
    }

    pub fn power(&mut self, _state: bool) {
    }

    pub fn nq_play_frame(&mut self, _frame: [u32; FIFO_DEPTH]) -> Result<(), [u32; DEPTH]> {
        Ok(())
    }
    pub fn dq_rec_frame(&mut self) -> Some([u32; FIFO_DEPTH]) {
        None
    }

    pub fn get_headset_code(&mut self) -> u8 {
        0
    }

    pub fn get_dacflag_code(&mut self) -> u8 {
        0
    }

    pub fn get_hp_status(&mut self) -> u8 {
        0
    }

    pub fn get_i2s_config(&mut self) -> [u8; 4] {
        [0, 0, 0, 0]
    }

    pub fn audio_clocks(&mut self) {
    }

    pub fn audio_ports(&mut self) {
    }

    pub fn audio_loopback(&mut self, _do_loop:bool) {
    }

    /// set up the audio mixer to sane defaults
    pub fn audio_mixer(&mut self) {
    }

    /// set up the betrusted-side signals
    pub fn audio_i2s_start(&mut self) {
    }

    pub fn audio_i2s_stop(&mut self) {
    }
}
