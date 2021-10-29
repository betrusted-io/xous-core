use com::Com;
use com::api::NET_MTU;

use smoltcp::Result;
use smoltcp::phy::{self, DeviceCapabilities, Medium};

use smoltcp::{
    time::Instant,
};

pub struct NetPhy {
    rx_buffer: [u8; NET_MTU],
    tx_buffer: [u8; NET_MTU],
    com: Com,
    rx_avail: Option<u16>,
}

impl<'a> NetPhy {
    pub fn new(xns: &xous_names::XousNames) -> NetPhy {
        NetPhy {
            rx_buffer: [0; NET_MTU],
            tx_buffer: [0; NET_MTU],
            com: Com::new(&xns).unwrap(),
            rx_avail: None,
        }
    }
    // returns None if there was a slot to put the availability into
    // returns Some(len) if not
    pub fn push_rx_avail(&mut self, len: u16) -> Option<u16> {
        if self.rx_avail.is_none() {
            self.rx_avail = Some(len);
            None
        } else {
            Some(len)
        }
    }
}

impl<'a> phy::Device<'a> for NetPhy {
    type RxToken = NetPhyRxToken<'a>;
    type TxToken = NetPhyTxToken<'a>;

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        if let Some(rx_len) = self.rx_avail.take() {
            self.com.wlan_fetch_packet(&mut self.rx_buffer[..rx_len as usize]).expect("Couldn't call wlan_fetch_packet in device adapter");

            Some((NetPhyRxToken{buf: &mut self.rx_buffer[..rx_len as usize]},
            NetPhyTxToken{buf: &mut self.tx_buffer[..], com: & self.com}))
        } else {
            None
        }
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        Some(NetPhyTxToken{buf: &mut self.tx_buffer[..], com: &self.com})
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = NET_MTU;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

pub struct NetPhyRxToken<'a> {
    buf: &'a mut [u8],
}

impl<'a, 'c> phy::RxToken for NetPhyRxToken<'a> {
    fn consume<R, F>(mut self, _timestamp: Instant, f: F) -> Result<R>
        where F: FnOnce(&mut [u8]) -> Result<R>
    {
        let result = f(&mut self.buf);
        //log::info!("rx: {:x?}", self.buf);
        result
    }
}

pub struct NetPhyTxToken<'a> {
    buf: &'a mut [u8],
    com: &'a Com,
}

impl<'a> phy::TxToken for NetPhyTxToken<'a> {
    fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> Result<R>
        where F: FnOnce(&mut [u8]) -> Result<R>
    {
        let result = f(&mut self.buf[..len]);
        //log::info!("txlen: {}", len);

        if result.is_ok() {
            self.com.wlan_send_packet(&self.buf[..len]).map_err(|_| smoltcp::Error::Dropped)?;
        }
        result
    }
}