#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use codec::FIFO_DEPTH;

pub struct Codec {
}

impl Codec {
    pub fn new(_conn: xous::CID, _xns: &xous_names::XousNames) -> Codec {
        Codec {
        }
    }
    pub fn suspend(&self) {
    }
    pub fn resume(&self) {
    }
    pub fn init(&mut self) {
    }

    pub fn nq_play_frame(&mut self, _frame: [u32; FIFO_DEPTH]) -> Result<(), [u32; FIFO_DEPTH]> {
        Ok(())
    }
    pub fn dq_rec_frame(&mut self) -> Option<[u32; FIFO_DEPTH]> {
        None
    }
    pub fn free_play_frames(&self) -> usize {
        0
    }

    pub fn can_play(&self) -> bool {
        false
    }

    pub fn drain(&mut self) {
    }

    pub fn available_rec_frames(&self) -> usize {
        0
    }

    pub fn power(&mut self, _state: bool) {
    }

    pub fn is_on(&self) -> bool {
        false
    }
    pub fn is_init(&self) -> bool {
        true
    }
    pub fn is_live(&self) -> bool {
        false
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

    pub fn set_speaker_gain_db(&mut self, _gain_db: f32) {
    }

    pub fn set_headphone_gain_db(&mut self, _gain_db_l: f32, _gain_db_r: f32) {
    }

}
