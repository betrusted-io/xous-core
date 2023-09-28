use com::Com;
use com::api::NET_MTU;

use smoltcp::phy::{self, DeviceCapabilities, Medium};
use smoltcp::wire::{ArpPacket, ArpRepr, ArpOperation, Ipv4Address, EthernetAddress, EthernetFrame, EthernetProtocol};
use num_traits::*;
use std::convert::TryInto;

use smoltcp::{
    time::Instant,
};

use crate::{MAC_ADDRESS_LSB, MAC_ADDRESS_MSB};
use core::sync::atomic::Ordering;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct NetPhy {
    rx_buffer: [u8; NET_MTU],
    tx_buffer: [u8; NET_MTU],
    com: Com,
    rx_avail: Option<u16>,
    loopback_conn: xous::CID,
    // tracks the length (and count) of the loopback packets pending
    loopback_pending: Arc::<Mutex::<VecDeque<u16>>>,
}

impl<'a> NetPhy {
    pub fn new(xns: &xous_names::XousNames, loopback_conn: xous::CID) -> NetPhy {
        NetPhy {
            rx_buffer: [0; NET_MTU],
            tx_buffer: [0; NET_MTU],
            com: Com::new(&xns).unwrap(),
            rx_avail: None,
            loopback_conn,
            loopback_pending: Arc::new(Mutex::new(VecDeque::new())),
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

impl phy::Device for NetPhy {
    type RxToken<'a> = NetPhyRxToken<'a>;
    type TxToken<'a> = NetPhyTxToken<'a>;

    fn receive(&mut self, _instant: smoltcp::time::Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Some(rx_len) = self.loopback_pending.lock().unwrap().pop_front() {
            // loopback takes precedence
            self.com.wlan_fetch_loopback_packet(&mut self.rx_buffer[..rx_len as usize]).expect("Couldn't call wlan_fetch_packet in device adapter");

            Some((NetPhyRxToken{buf: &mut self.rx_buffer[..rx_len as usize]},
            NetPhyTxToken{buf: &mut self.tx_buffer[..], com: & self.com, loopback_conn: self.loopback_conn, loopback_count: self.loopback_pending.clone()}))
        } else {
            if let Some(rx_len) = self.rx_avail.take() {
                self.com.wlan_fetch_packet(&mut self.rx_buffer[..rx_len as usize]).expect("Couldn't call wlan_fetch_packet in device adapter");

                Some((NetPhyRxToken{buf: &mut self.rx_buffer[..rx_len as usize]},
                NetPhyTxToken{buf: &mut self.tx_buffer[..], com: & self.com, loopback_conn: self.loopback_conn, loopback_count: self.loopback_pending.clone()}))
            } else {
                None
            }
        }
    }

    fn transmit(&mut self, _instant: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        Some(NetPhyTxToken{buf: &mut self.tx_buffer[..], com: &self.com, loopback_conn: self.loopback_conn, loopback_count: self.loopback_pending.clone()})
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
    fn consume<R, F>(mut self, f: F) -> R
        where F: FnOnce(&mut [u8]) -> R
    {
        let result = f(&mut self.buf);
        //log::info!("rx: {:x?}", self.buf);
        result
    }
}

pub struct NetPhyTxToken<'a> {
    buf: &'a mut [u8],
    com: &'a Com,
    loopback_conn: xous::CID,
    loopback_count: Arc::<Mutex::<VecDeque<u16>>>,
}
impl <'a> NetPhyTxToken<'a> {
    /// Initiates the Rx side of things to read out the loopback packet that was queued
    fn loopback_rx(&self, rxlen: usize) {
        self.loopback_count.lock().unwrap().push_back(rxlen as u16);
        xous::try_send_message(self.loopback_conn,
            xous::Message::new_scalar(crate::Opcode::LoopbackRx.to_usize().unwrap(), rxlen, 0, 0, 0)
        ).ok();
    }

    // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this issue as well:
    // https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
    // this function creates the ARP packets for injection
    fn wlan_queue_localhost_arp(&self,
        target_mac: &[u8; 6],
        target_addr: Ipv4Address,
        remote_hw_addr: EthernetAddress,
        remote_ip_addr: Ipv4Address,
    ) {
        let mut eth_bytes = vec![0u8; 42];

        //let remote_ip_addr = Ipv4Address([127, 0, 0, 1]);
        //let remote_hw_addr = EthernetAddress([0, 0, 0, 0, 0, 0]);

        let repr = ArpRepr::EthernetIpv4 {
            operation: ArpOperation::Reply,
            source_hardware_addr: remote_hw_addr,
            source_protocol_addr: remote_ip_addr,
            target_hardware_addr: EthernetAddress(target_mac.clone()),
            target_protocol_addr: target_addr,
        };

        let mut frame = EthernetFrame::new_unchecked(&mut eth_bytes);
        frame.set_dst_addr(EthernetAddress(target_mac.clone()));
        frame.set_src_addr(remote_hw_addr);
        frame.set_ethertype(EthernetProtocol::Arp);
        {
            let mut packet = ArpPacket::new_unchecked(frame.payload_mut());
            repr.emit(&mut packet);
        }
        let pkt = frame.into_inner().to_vec();
        log::debug!("stuffing arp {:?}", pkt);
        self.com.wlan_queue_loopback(&pkt);
        self.loopback_rx(pkt.len());
    }
}

impl<'a> phy::TxToken for NetPhyTxToken<'a> {
    fn consume<R, F>(mut self, len: usize, f: F) -> R
        where F: FnOnce(&mut [u8]) -> R
    {
        let result = f(&mut self.buf[..len]);
        //log::info!("txlen: {}", len);

        {
            // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this issue as well:
            // https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
            // detect an outbound loopback packet, and shove it back in
            let mut loopback = false;
            if let Ok(mut frame) = smoltcp::wire::EthernetFrame::new_checked(&mut self.buf) {
                let mut local_hwaddr = [0u8; 6];
                local_hwaddr[2..6].copy_from_slice(&MAC_ADDRESS_LSB.load(Ordering::SeqCst).to_be_bytes());
                local_hwaddr[0..2].copy_from_slice(&MAC_ADDRESS_MSB.load(Ordering::SeqCst).to_be_bytes());
                if frame.dst_addr() == EthernetAddress::default() {
                    log::debug!("loopback packet redirect");
                    // override the destination address to be our own address
                    frame.set_dst_addr(smoltcp::wire::EthernetAddress(local_hwaddr));
                    loopback = true;
                } else if frame.dst_addr().as_bytes() == local_hwaddr {
                    log::debug!("loopback return packet redirect");
                    loopback = true;
                }
            }
            // NOTE: was protected by if result.is_ok(), but looking at smoltcp, the calling routines
            // *always* return OK, or nothing. So we are removing the protecting .is_ok()...
            if loopback {
                self.com.wlan_queue_loopback(&self.buf[..len]);
                self.loopback_rx(len);
            } else {
                {
                    // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this issue as well:
                    // https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
                    // this part of the hack finds ARP packets to and from the local device, and responds to them with packet injections
                    let pkt = &self.buf[..len];
                    if let Ok(frame) = EthernetFrame::new_checked(pkt) {
                        if frame.ethertype() == EthernetProtocol::Arp {
                            // we're parsing packets that came from our own stack -- they better be OK!
                            let arp_packet = ArpPacket::new_checked(frame.payload()).unwrap();
                            let arp_repr = ArpRepr::parse(&arp_packet).unwrap();
                            match arp_repr {
                                ArpRepr::EthernetIpv4 {
                                    operation, source_hardware_addr, source_protocol_addr, target_protocol_addr, ..
                                } => {
                                    log::trace!("outgoing arp: {:?} {:?} {:?}", source_hardware_addr, source_protocol_addr, target_protocol_addr);
                                    if target_protocol_addr.as_bytes() == [127, 0, 0, 1] && operation == ArpOperation::Request {
                                        log::trace!("intercepted outgoing arp for 127.0.0.1: {:?}", pkt);
                                        self.wlan_queue_localhost_arp(
                                            source_hardware_addr.as_bytes().try_into().unwrap(),
                                            source_protocol_addr,
                                            EthernetAddress([0, 0, 0, 0, 0, 0,]),
                                            Ipv4Address([127, 0, 0, 1]),
                                        );
                                        return result;
                                    } else if source_protocol_addr.as_bytes() == [127, 0, 0, 1] && operation == ArpOperation::Request {
                                        // reverse lookup case
                                        log::trace!("intercepted outgoing arp for own IP: {:?} {:?}", target_protocol_addr, pkt);
                                        self.wlan_queue_localhost_arp(
                                            source_hardware_addr.as_bytes().try_into().unwrap(),
                                            source_protocol_addr,
                                            source_hardware_addr,
                                            target_protocol_addr,
                                        );
                                        return result;
                                    } else {
                                        // don't do anything, pass it on
                                    }
                                }
                                _ => {}, // pass it on
                            }
                        }
                    }
                }

                // normally this would be the only line!
                self.com.wlan_send_packet(&self.buf[..len]).expect("driver error sending WLAN packet");
            }
        }
        result
    }
}