use core::sync::atomic::Ordering;
use std::collections::VecDeque;
use std::convert::TryInto;
use std::sync::{Arc, Mutex};

use com::api::NET_MTU;
use com::Com;
use num_traits::*;
use smoltcp::phy::{self, ChecksumCapabilities, DeviceCapabilities, Medium};
use smoltcp::wire::{
    ArpOperation, ArpPacket, ArpRepr, EthernetAddress, EthernetFrame, EthernetProtocol, Ipv4Address,
    Ipv4Packet, Ipv4Repr, /* IpProtocol, TcpPacket, TcpRepr, IpAddress, UdpPacket, UdpRepr */
};

use crate::{IPV4_ADDRESS, MAC_ADDRESS_LSB, MAC_ADDRESS_MSB};

pub struct NetPhy {
    rx_buffer: [u8; NET_MTU],
    tx_buffer: [u8; NET_MTU],
    com: Com,
    rx_avail: Option<u16>,
    loopback_conn: xous::CID,
    // tracks the length (and count) of the loopback packets pending
    loopback_pending: Arc<Mutex<VecDeque<u16>>>,
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

    fn receive(
        &mut self,
        _instant: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let csum_copy = self.capabilities().checksum.clone();
        if let Some(rx_len) = self.loopback_pending.lock().unwrap().pop_front() {
            log::debug!("loopback injected {} bytes", rx_len);
            // loopback takes precedence
            self.com
                .wlan_fetch_loopback_packet(&mut self.rx_buffer[..rx_len as usize])
                .expect("Couldn't call wlan_fetch_packet in device adapter");

            Some((
                NetPhyRxToken { buf: &mut self.rx_buffer[..rx_len as usize] },
                NetPhyTxToken {
                    buf: &mut self.tx_buffer[..],
                    com: &self.com,
                    loopback_conn: self.loopback_conn,
                    loopback_count: self.loopback_pending.clone(),
                    caps: csum_copy,
                },
            ))
        } else {
            if let Some(rx_len) = self.rx_avail.take() {
                log::debug!("device rx of {} bytes", rx_len);
                self.com
                    .wlan_fetch_packet(&mut self.rx_buffer[..rx_len as usize])
                    .expect("Couldn't call wlan_fetch_packet in device adapter");

                Some((
                    NetPhyRxToken { buf: &mut self.rx_buffer[..rx_len as usize] },
                    NetPhyTxToken {
                        buf: &mut self.tx_buffer[..],
                        com: &self.com,
                        loopback_conn: self.loopback_conn,
                        loopback_count: self.loopback_pending.clone(),
                        caps: csum_copy,
                    },
                ))
            } else {
                log::trace!("nothing to rx");
                None
            }
        }
    }

    fn transmit(&mut self, _instant: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        let csum_copy = self.capabilities().checksum.clone();
        log::debug!("device tx");
        Some(NetPhyTxToken {
            buf: &mut self.tx_buffer[..],
            com: &self.com,
            loopback_conn: self.loopback_conn,
            loopback_count: self.loopback_pending.clone(),
            caps: csum_copy,
        })
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
    where
        F: FnOnce(&mut [u8]) -> R,
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
    loopback_count: Arc<Mutex<VecDeque<u16>>>,
    caps: ChecksumCapabilities,
}
impl<'a> NetPhyTxToken<'a> {
    /// Initiates the Rx side of things to read out the loopback packet that was queued
    fn loopback_rx(&self, rxlen: usize) {
        // this will initiate a target trace
        // log::set_max_level(log::LevelFilter::Trace);
        self.loopback_count.lock().unwrap().push_back(rxlen as u16);
        xous::try_send_message(
            self.loopback_conn,
            xous::Message::new_scalar(crate::Opcode::LoopbackRx.to_usize().unwrap(), rxlen, 0, 0, 0),
        )
        .ok();
    }

    // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking this issue
    // as well: https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
    // this function creates the ARP packets for injection
    fn wlan_queue_localhost_arp(
        &self,
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
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(&mut self.buf[..len]);
        log::debug!("txlen: {}", len);

        {
            // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but tracking
            // this issue as well: https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
            // detect an outbound loopback packet, and shove it back in
            if let Ok(mut eth_frame) = EthernetFrame::new_checked(&mut self.buf[..len]) {
                let mut local_hwaddr = [0u8; 6];
                local_hwaddr[2..6].copy_from_slice(&MAC_ADDRESS_LSB.load(Ordering::SeqCst).to_be_bytes());
                local_hwaddr[0..2].copy_from_slice(&MAC_ADDRESS_MSB.load(Ordering::SeqCst).to_be_bytes());
                if eth_frame.dst_addr() == EthernetAddress::default() {
                    log::debug!("loopback packet redirect");
                    // override the destination address to be our own address
                    eth_frame.set_dst_addr(smoltcp::wire::EthernetAddress(local_hwaddr));
                    self.com.wlan_queue_loopback(&self.buf[..len]);
                    self.loopback_rx(len);
                    return result;
                } else if eth_frame.dst_addr().as_bytes() == local_hwaddr {
                    log::debug!("loopback return packet redirect");
                    self.com.wlan_queue_loopback(&self.buf[..len]);
                    self.loopback_rx(len);
                    return result;
                }
                // make a copy of the packet so we can meet mutability requirements of the routines below...
                let mut packet_clone = [0u8; NET_MTU];
                let payload_len = eth_frame.payload_mut().len();
                packet_clone[..payload_len].copy_from_slice(&eth_frame.payload_mut()[..payload_len]);
                match eth_frame.ethertype() {
                    EthernetProtocol::Ipv4 => {
                        if let Ok(packet) = Ipv4Packet::new_checked(&packet_clone[..payload_len]) {
                            log::debug!(
                                "IPV4 packet checksum: {:x} ({:?})",
                                packet.checksum(),
                                packet.verify_checksum()
                            );
                            if let Ok(ipv4_parsed) = Ipv4Repr::parse(&packet, &self.caps) {
                                log::debug!("IPV4 parsed: {:?}", ipv4_parsed);
                                if ipv4_parsed.dst_addr.as_bytes() == [127, 0, 0, 1] {
                                    log::debug!("patching destination 127.0.0.1");
                                    // patch the destination MAC address
                                    let mut local_hwaddr = [0u8; 6];
                                    local_hwaddr[2..6].copy_from_slice(
                                        &MAC_ADDRESS_LSB.load(Ordering::SeqCst).to_be_bytes(),
                                    );
                                    local_hwaddr[0..2].copy_from_slice(
                                        &MAC_ADDRESS_MSB.load(Ordering::SeqCst).to_be_bytes(),
                                    );
                                    eth_frame.set_dst_addr(smoltcp::wire::EthernetAddress(local_hwaddr));

                                    /* this is no longer necessary in smoltcp 0.9 or later - an interface can bind to multiple sockets, one of which being 127.0.0.1
                                    // patch the destination IP address
                                    let orig_dst_addr = ipv4_parsed.dst_addr;
                                    ipv4_parsed.dst_addr = smoltcp::wire::Ipv4Address::from_bytes(&IPV4_ADDRESS.load(Ordering::SeqCst).to_be_bytes());

                                    // extract the buffer region from the original frame and overwrite it
                                    let payload_buf = eth_frame.payload_mut();
                                    let mut mut_pkt = Ipv4Packet::new_unchecked(payload_buf);
                                    // recompute inner-checksums for TCP and UDP cases...
                                    match ipv4_parsed.next_header {
                                        IpProtocol::Tcp => {
                                            let tcp_packet = TcpPacket::new_unchecked(packet.payload());
                                            if let Ok(tcp_repr) = TcpRepr::parse(
                                                &tcp_packet,
                                                &IpAddress::Ipv4(ipv4_parsed.src_addr),
                                                &IpAddress::Ipv4(orig_dst_addr),
                                                &self.caps
                                            ) {
                                                log::trace!("tcp checksum: {:x}", tcp_packet.checksum());
                                                let mut mut_tcp_packet = TcpPacket::new_unchecked(mut_pkt.payload_mut());
                                                tcp_repr.emit(
                                                    &mut mut_tcp_packet,
                                                    &IpAddress::Ipv4(ipv4_parsed.src_addr),
                                                    &IpAddress::Ipv4(ipv4_parsed.dst_addr),
                                                    &self.caps
                                                );
                                                log::trace!("tcp new checksum: {:x}", tcp_packet.checksum());
                                            } else {
                                                log::error!("Transmitted TCP packet did not unpack correctly! checksum: {:?}", tcp_packet.verify_checksum(
                                                    &IpAddress::Ipv4(ipv4_parsed.src_addr), &IpAddress::Ipv4(orig_dst_addr)));
                                            }
                                        }
                                        IpProtocol::Udp => {
                                            let udp_packet = UdpPacket::new_unchecked(packet.payload());
                                            if let Ok(udp_repr) = UdpRepr::parse(
                                                &udp_packet,
                                                &IpAddress::Ipv4(ipv4_parsed.src_addr),
                                                &IpAddress::Ipv4(orig_dst_addr),
                                                &self.caps
                                            ) {
                                                log::debug!("udp checksum: {:x}", udp_packet.checksum());
                                                let mut mut_udp_packet = UdpPacket::new_unchecked(mut_pkt.payload_mut());
                                                udp_repr.emit(
                                                    &mut mut_udp_packet,
                                                    &IpAddress::Ipv4(ipv4_parsed.src_addr),
                                                    &IpAddress::Ipv4(ipv4_parsed.dst_addr),
                                                    udp_packet.len() as usize,
                                                    |buf| buf.copy_from_slice(packet.payload()),
                                                    &self.caps
                                                );
                                                log::debug!("udp new checksum: {:x}", udp_packet.checksum());
                                            } else {
                                                log::error!("Transmitted UDP packet did not unpack correctly! checksum: {:?}", udp_packet.verify_checksum(
                                                    &IpAddress::Ipv4(ipv4_parsed.src_addr), &IpAddress::Ipv4(orig_dst_addr)));
                                            }
                                        }
                                        _ => {
                                            log::warn!("Unhandled packet type in loopback! Checksum will be incorrect.");
                                        }
                                    }

                                    log::trace!("checksum: {:x}", mut_pkt.checksum());
                                    ipv4_parsed.emit(&mut mut_pkt, &self.caps);
                                    log::trace!("new checksum: {:x}", mut_pkt.checksum());
                                    */

                                    let buf_to_send = eth_frame.into_inner();
                                    self.com.wlan_queue_loopback(buf_to_send);
                                    /* // for double-checking the RX packet
                                        if let Ok(check_frame) = EthernetFrame::new_checked(&buf_to_send) {
                                            if let Ok(check_pkt) = Ipv4Packet::new_checked(&check_frame.payload()) {
                                                if let Ok(check_parsed) = Ipv4Repr::parse(&check_pkt, &self.caps) {
                                                    log::debug!("CHECK: checksum: {:x}, valid: {:?}, patched packet: {:?}",
                                                    check_pkt.checksum(),
                                                    check_pkt.verify_checksum(),
                                                    check_parsed);
                                                } else {
                                                    log::debug!("CHECK: FAILED - invalid IPv4 packet");
                                                }
                                            } else {
                                                log::debug!("CHECK: FAILED - invalid payload");
                                            }
                                        } else {
                                            log::debug!("CHECK: FAILED - invalid ethernet frame");
                                        }
                                    */
                                    self.loopback_rx(len);
                                    // exit here without sending the packet on, because it went to Rx
                                    return result;
                                }
                            }
                        }
                    }
                    EthernetProtocol::Arp => {
                        // this is a hack to make loopbacks work on smoltcp. Work-around taken from Redox, but
                        // tracking this issue as well: https://github.com/smoltcp-rs/smoltcp/issues/50 and https://github.com/smoltcp-rs/smoltcp/issues/55
                        // this part of the hack finds ARP packets to and from the local device, and responds
                        // to them with packet injections we're parsing packets that
                        // came from our own stack -- they better be OK!
                        let arp_packet = ArpPacket::new_checked(eth_frame.payload_mut()).unwrap();
                        let arp_repr = ArpRepr::parse(&arp_packet).unwrap();
                        match arp_repr {
                            ArpRepr::EthernetIpv4 {
                                operation,
                                source_hardware_addr,
                                source_protocol_addr,
                                target_protocol_addr,
                                ..
                            } => {
                                log::debug!("outgoing arp: {:?} {:?} {:?}", source_hardware_addr, source_protocol_addr, target_protocol_addr);
                                let local_addr = IPV4_ADDRESS.load(Ordering::SeqCst).to_be_bytes();
                                if (target_protocol_addr.as_bytes() == [127, 0, 0, 1]
                                || target_protocol_addr.as_bytes() == local_addr)
                                && operation == ArpOperation::Request {
                                    log::debug!("intercepted outgoing arp for localhost: {:?}", arp_repr);
                                    let mut local_hwaddr = [0u8; 6];
                                    local_hwaddr[2..6].copy_from_slice(&MAC_ADDRESS_LSB.load(Ordering::SeqCst).to_be_bytes());
                                    local_hwaddr[0..2].copy_from_slice(&MAC_ADDRESS_MSB.load(Ordering::SeqCst).to_be_bytes());
                                    self.wlan_queue_localhost_arp(
                                        source_hardware_addr.as_bytes().try_into().unwrap(),
                                        source_protocol_addr,
                                        EthernetAddress(local_hwaddr),
                                        Ipv4Address(target_protocol_addr.as_bytes().try_into().unwrap()),
                                    );
                                    return result;
                                } /* else if (source_protocol_addr.as_bytes() == [127, 0, 0, 1]
                                || source_protocol_addr.as_bytes() == local_addr)
                                && operation == ArpOperation::Request {
                                    // reverse lookup case
                                    log::debug!("intercepted outgoing arp for own IP: {:?} {:?}", target_protocol_addr, arp_repr);
                                    self.wlan_queue_localhost_arp(
                                        source_hardware_addr.as_bytes().try_into().unwrap(),
                                        source_protocol_addr,
                                        source_hardware_addr,
                                        target_protocol_addr,
                                    );
                                    return result;
                                } else {
                                    // don't do anything, pass it on
                                } */
                            }
                            _ => {} // pass it on
                        }
                    }
                    _ => {}
                }
            }
        }
        // forward the packet on if it's not a loopback (loopback will call return early and exit before
        // getting to this line)
        self.com.wlan_send_packet(&self.buf[..len]).expect("driver error sending WLAN packet");

        result
    }
}
