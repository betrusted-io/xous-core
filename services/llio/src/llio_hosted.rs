// a stub to try to avoid breaking hosted mode for as long as possible.
use llio::api::*;

#[derive(Copy, Clone, Debug)]
pub struct Llio {
}
pub fn log_init() -> *mut u32 { 0 as *mut u32 }

impl Llio {
    pub fn new(_handler_conn: xous::CID, _gpio_base: *mut u32) -> Llio {
        Llio {
        }
    }
    pub fn suspend(&self) {}
    pub fn resume(&self) {}
    pub fn gpio_dout(&self, _d: u32) {}
    pub fn gpio_din(&self, ) -> u32 { 0xDEAD_BEEF }
    pub fn gpio_drive(&self, _d: u32) {}
    pub fn gpio_int_mask(&self, _d: u32) {}
    pub fn gpio_int_as_falling(&self, _d: u32) {}
    pub fn gpio_int_pending(&self, ) -> u32 { 0x0 }
    pub fn gpio_int_ena(&self, _d: u32) {}
    pub fn set_uart_mux(&self, _mux: UartType) {}
    #[cfg(feature="test-rekey")]
    pub fn get_info_dna(&self, ) ->  (usize, usize) { (0, 1) }
    #[cfg(not(feature="test-rekey"))]
    pub fn get_info_dna(&self, ) ->  (usize, usize) { (0, 0) }
    pub fn get_info_git(&self, ) ->  (usize, usize) { (0, 0) }
    pub fn get_info_platform(&self, ) ->  (usize, usize) { (0, 0) }
    pub fn get_info_target(&self, ) ->  (usize, usize) { (0, 0) }
    pub fn power_audio(&self, _power_on: bool) {}
    pub fn power_crypto(&self, _power_on: bool) {}
    pub fn power_crypto_status(&self) -> (bool, bool, bool, bool) {
        (true, true, true, true)
    }
    pub fn power_self(&self, _power_on: bool) {}
    pub fn power_boost_mode(&self, _power_on: bool) {}
    pub fn ec_snoop_allow(&self, _power_on: bool) {}
    pub fn ec_reset(&self, ) {}
    pub fn ec_power_on(&self, ) {}
    pub fn self_destruct(&self, _code: u32) {}
    pub fn vibe(&self, pattern: VibePattern) {
        log::info!("Imagine your keyboard vibrating: {:?}", pattern);
    }


    pub fn xadc_vbus(&self) -> u16 {
        2 // some small but non-zero value to represent typical noise
    }
    pub fn xadc_vccint(&self) -> u16 {
        1296
    }
    pub fn xadc_vccaux(&self) -> u16 {
        2457
    }
    pub fn xadc_vccbram(&self) -> u16 {
        2450
    }
    pub fn xadc_usbn(&self) -> u16 {
        3
    }
    pub fn xadc_usbp(&self) -> u16 {
        4
    }
    pub fn xadc_temperature(&self) -> u16 {
        2463
    }
    pub fn xadc_gpio5(&self) -> u16 {
        0
    }
    pub fn xadc_gpio2(&self) -> u16 {
        0
    }

    pub fn com_int_ena(self, _ena: bool) {
    }
    pub fn usb_int_ena(self, _ena: bool) {
    }
    pub fn debug_powerdown(&mut self, _ena: bool) {
    }
    pub fn debug_wakeup(&mut self, _ena: bool) {
    }
    #[allow(dead_code)]
    pub fn activity_get_period(&mut self) -> u32 {
        12_000_000
    }
    pub fn wfi_override(&mut self, _override_: bool) {
    }

    #[allow(dead_code)]
    pub fn tts_sleep_indicate(&mut self) {
    }
    pub fn get_power_csr_raw(&self) -> u32 {
        0
    }
}
