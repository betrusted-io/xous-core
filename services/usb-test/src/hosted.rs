pub struct UsbTest {}

impl UsbTest {
    pub fn new() -> UsbTest { UsbTest {} }

    pub fn suspend(&self) {}

    pub fn resume(&self) {}
}

use keyboard::{KeyMap, KeyRawStates, RowCol, ScanCode};
#[allow(dead_code)]
pub(crate) struct Keyboard {
    cid: xous::CID,
    map: KeyMap,
    rate: u32,
    delay: u32,
    chord_interval: u32,
    pub debug: u32,
}

impl Keyboard {
    pub fn new(sid: xous::SID) -> Keyboard {
        Keyboard {
            cid: xous::connect(sid).unwrap(),
            map: KeyMap::Qwerty,
            rate: 20,
            delay: 200,
            chord_interval: 50,
            debug: 0,
        }
    }

    pub fn suspend(&self) {}

    pub fn resume(&self) {}

    pub fn set_map(&mut self, map: KeyMap) { self.map = map; }

    pub fn get_map(&self) -> KeyMap { self.map }

    pub fn update(&self) -> KeyRawStates { KeyRawStates::new() }

    pub fn track_chord(&mut self, _krs: &KeyRawStates) -> Vec<char> { Vec::new() }

    pub fn track_keys(&mut self, _rs: &KeyRawStates) -> Vec<char> { Vec::new() }

    pub fn set_repeat(&mut self, rate: u32, delay: u32) {
        self.rate = rate;
        self.delay = delay;
    }

    pub fn set_chord_interval(&mut self, delay: u32) { self.chord_interval = delay; }

    pub fn is_repeating_key(&self) -> bool { false }

    pub(crate) fn get_repeat_check_interval(&self) -> u32 { self.rate }

    pub(crate) fn poll(&mut self) {}
}

pub struct SpinalUsbDevice {}

impl SpinalUsbDevice {
    pub fn new(_sid: xous::SID) -> SpinalUsbDevice { SpinalUsbDevice {} }

    pub fn print_regs(&self) {}

    /// simple but easy to understand allocator for buffers inside the descriptor memory space
    pub fn alloc_region(&mut self, requested: usize) -> Option<u32> { None }

    /// returns `true` if the region was available to be deallocated
    pub fn dealloc_region(&mut self, offset: usize) -> bool { false }

    pub fn connect_device_core(&mut self, _state: bool) {}

    pub fn suspend(&mut self) {}

    pub fn resume(&mut self) {}
}
