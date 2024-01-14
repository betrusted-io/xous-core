#![allow(dead_code)]

pub struct SpinalUdcDescriptor {}

pub struct UdcEpStatus {}

pub struct SpinalUsbMgmt {}
impl SpinalUsbMgmt {
    pub fn print_regs(&self) {}

    pub fn connect_device_core(&mut self, _state: bool) {}

    pub fn is_device_connected(&self) -> bool { false }

    pub fn disable_debug(&mut self, _disable: bool) {}

    pub fn get_disable_debug(&self) -> bool { false }

    pub fn xous_suspend(&mut self) {}

    pub fn xous_resume1(&mut self) {}

    pub fn xous_resume2(&mut self) {}

    pub fn descriptor_from_status(&self, _ep_status: &UdcEpStatus) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor {}
    }

    pub fn status_from_index(&self, _index: usize) -> UdcEpStatus { UdcEpStatus {} }
}
pub struct SpinalUsbDevice {}

impl SpinalUsbDevice {
    pub fn new(_sid: xous::SID, _view: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> SpinalUsbDevice {
        SpinalUsbDevice {}
    }

    pub fn get_iface(&self) -> SpinalUsbMgmt { SpinalUsbMgmt {} }

    pub fn print_ep_stats(&self) {}

    pub fn print_regs(&self) {}

    /// simple but easy to understand allocator for buffers inside the descriptor memory space
    pub fn alloc_region(&mut self, _requested: usize) -> Option<u32> { None }

    /// returns `true` if the region was available to be deallocated
    pub fn dealloc_region(&mut self, _offset: usize) -> bool { false }
}
