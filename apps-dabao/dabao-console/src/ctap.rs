use usb_bao1x::RawFidoReport;

pub struct XousHidConnection {
    pub endpoint: usb_bao1x::UsbHid,
}
impl XousHidConnection {
    pub fn u2f_wait_incoming(&self) -> Result<RawFidoReport, xous::Error> {
        self.endpoint.u2f_wait_incoming()
    }

    pub fn u2f_send(&self, msg: RawFidoReport) -> Result<(), xous::Error> { self.endpoint.u2f_send(msg) }
}

/// Barest-minimum test to do INIT and PING response as a HID responder.
pub fn ctap_test() {
    let _ = std::thread::spawn({
        move || {
            let endpoint = usb_bao1x::UsbHid::new();
            let hid_connection = XousHidConnection { endpoint };
            let mut errcnt = 0;
            loop {
                log::debug!("wait for incoming u2f");
                match hid_connection.u2f_wait_incoming() {
                    Ok(msg) => {
                        log::info!("got incoming {:x?}", msg);
                        let payload = msg.packet;
                        let mut resp = RawFidoReport::default();
                        if payload[4] == 0x86 {
                            resp.packet[0..7].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0x86, 0x0, 0x11]);
                            resp.packet[7..7 + 8].copy_from_slice(&payload[7..15]);
                            resp.packet[15..15 + 9]
                                .copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x02, 0x05, 0x01, 0x2a, 0x01]);
                            log::info!("responding {:x?}", resp);
                            hid_connection.u2f_send(resp).ok();
                        }
                        if payload[4] == 0x81 {
                            resp.packet[0..7].copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x81, 0x00, 0x8]);
                            resp.packet[7..7 + 8].copy_from_slice(&payload[7..15]);
                            log::info!("responding {:x?}", resp);
                            hid_connection.u2f_send(resp).ok();
                        }
                    }
                    Err(e) => {
                        if errcnt < 8 {
                            log::info!("error waiting {:?}", e);
                        }
                        errcnt += 1;
                    }
                }
            }
        }
    });
}
