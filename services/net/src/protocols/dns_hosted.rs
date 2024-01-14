use std::io::Result;
use std::net::{IpAddr, Ipv4Addr};

pub struct DnsServerManager {}

impl DnsServerManager {
    pub fn register(_xns: &xous_names::XousNames) -> Result<DnsServerManager> { Ok(DnsServerManager {}) }

    /// Fake function that always returns true
    pub fn add_server(&mut self, _addr: IpAddr) -> bool { true }

    /// Fake function that always returns true
    pub fn remove_server(&mut self, _addr: IpAddr) -> bool { true }

    /// Fake function
    pub fn clear(&mut self) {}

    /// Fake function
    pub fn set_freeze(&mut self, _freeze: bool) {}

    /// Always returns 1.1.1.1
    pub fn get_random(&self) -> Option<IpAddr> { Some(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))) }
}
