use std::net::ToSocketAddrs;

use net::NetIpAddr;

use crate::DnsResponseCode;

#[derive(Debug)]
pub struct Dns {}
impl Dns {
    pub fn new(_xns: &xous_names::XousNames) -> Result<Self, xous::Error> { Ok(Dns {}) }

    /// Checks first to see if the name could be just an IPv4 or IPv6 in string form,
    /// then tries to pass it to the DNS resolver.
    pub fn lookup(&self, name: &str) -> Result<NetIpAddr, DnsResponseCode> {
        log::debug!("looking up {}", name);
        match (name, 80).to_socket_addrs() {
            // we throw away the port because we just want the IP address...
            Ok(mut iter) => match iter.next() {
                Some(addr) => {
                    log::debug!("{:?}", addr);
                    Ok(NetIpAddr::from(addr))
                }
                None => {
                    log::debug!("name error");
                    Err(DnsResponseCode::NameError)
                }
            },
            Err(e) => {
                log::debug!("format error: {:?}", e);
                Err(DnsResponseCode::FormatError)
            }
        }
    }

    pub fn flush_cache(&self) -> Result<(), xous::Error> {
        log::warn!("DNS cache flush not implemented in hosted mode!");
        Ok(())
    }
}
