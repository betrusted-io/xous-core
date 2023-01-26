use core::convert::TryInto;
use num_traits::*;
use std::sync::Arc;
use std::sync::Mutex;
use usbd_scsi::BlockDevice;
use xous::msg_blocking_scalar_unpack;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum BlockDeviceMgmtOp {
    SetOps,
    SetSID,
    Reset,
}

#[repr(C, align(4096))]
pub struct RawData {
    raw: [u8; 4096],
}

#[derive(Debug)]
struct RwOp {
    read_id: usize,
    write_id: usize,
    max_lba_id: usize,
}

pub struct AppsBlockDevice {
    app_cid: Arc<Mutex<Option<xous::CID>>>,
    rw_ids: Arc<Mutex<RwOp>>,
    mgmt_cid: xous::CID,
}

impl AppsBlockDevice {
    pub fn new() -> AppsBlockDevice {
        let app_cid = Arc::new(Mutex::new(None));
        let rw_ids = Arc::new(Mutex::new(RwOp {
            read_id: 0,
            write_id: 0,
            max_lba_id: 0,
        }));

        let app_cid_clone = app_cid.clone();
        let rw_ids_clone = rw_ids.clone();

        let mgmt_sid = xous::create_server().unwrap();

        std::thread::spawn(move || loop {
            let msg = xous::receive_message(mgmt_sid).unwrap();
            let opcode: Option<BlockDeviceMgmtOp> = FromPrimitive::from_usize(msg.body.id());
            match opcode {
                Some(BlockDeviceMgmtOp::SetOps) => {
                    msg_blocking_scalar_unpack!(msg, read_id, write_id, max_lba_id, _, {
                        let mut ids = rw_ids_clone.lock().unwrap();

                        *ids = RwOp {
                            read_id,
                            write_id,
                            max_lba_id,
                        };

                        log::info!("setting new app block device handler: {:?}", ids);
                        xous::return_scalar(msg.sender, 0).unwrap();
                    })
                }
                Some(BlockDeviceMgmtOp::SetSID) => {
                    msg_blocking_scalar_unpack!(msg, sid1, sid2, sid3, sid4, {
                        let app_sid =
                            xous::SID::from_u32(sid1 as u32, sid2 as u32, sid3 as u32, sid4 as u32);
                        let app_cid = xous::connect(app_sid).unwrap();
                        let mut ac = app_cid_clone.lock().unwrap();

                        *ac = Some(app_cid as u32);
                        log::info!("setting new app block device handler SID: {:?}", app_sid);
                        xous::return_scalar(msg.sender, 0).unwrap();
                    })
                }
                Some(BlockDeviceMgmtOp::Reset) => msg_blocking_scalar_unpack!(msg, 0, 0, 0, 0, {
                    let mut ac = app_cid_clone.lock().unwrap();
                    let mut ids = rw_ids_clone.lock().unwrap();

                    *ac = None;
                    *ids = RwOp {
                        read_id: 0,
                        write_id: 0,
                        max_lba_id: 0,
                    };

                    xous::return_scalar(msg.sender, 0).unwrap();
                }),
                None => unimplemented!("missing opcode for appsblockdevice mgmt interface"),
            }
        });

        AppsBlockDevice {
            app_cid,
            rw_ids,
            mgmt_cid: xous::connect(mgmt_sid).unwrap(),
        }
    }
    pub fn conn(&self) -> xous::CID {
        self.mgmt_cid
    }
}

impl BlockDevice for AppsBlockDevice {
    const BLOCK_BYTES: usize = 512;

    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        let cid = match *self.app_cid.lock().unwrap() {
            Some(cid) => cid,
            None => return Err(usbd_scsi::BlockDeviceError::HardwareError),
        };

        let rw_ids = self.rw_ids.lock().unwrap();

        let mut request = RawData { raw: [0u8; 4096] };

        let buf = unsafe {
            xous::MemoryRange::new(
                &mut request as *mut RawData as usize,
                core::mem::size_of::<RawData>(),
            )
            .unwrap()
        };

        xous::send_message(
            cid,
            xous::Message::new_lend_mut(
                rw_ids.read_id,
                buf,
                xous::MemoryAddress::new(lba as usize),
                None,
            ),
        )
        .unwrap();

        block.copy_from_slice(&request.raw[..Self::BLOCK_BYTES as usize]);

        Ok(())
    }

    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        let cid = match *self.app_cid.lock().unwrap() {
            Some(cid) => cid,
            None => return Err(usbd_scsi::BlockDeviceError::HardwareError),
        };

        let rw_ids = self.rw_ids.lock().unwrap();

        let mut request = RawData { raw: [0u8; 4096] };
        (request.raw[..Self::BLOCK_BYTES as usize]).copy_from_slice(block);

        let buf = unsafe {
            xous::MemoryRange::new(
                &mut request as *mut RawData as usize,
                core::mem::size_of::<RawData>(),
            )
            .unwrap()
        };

        xous::send_message(
            cid,
            xous::Message::new_lend_mut(
                rw_ids.write_id,
                buf,
                xous::MemoryAddress::new(lba as usize),
                None,
            ),
        )
        .unwrap();

        Ok(())
    }

    fn max_lba(&self) -> u32 {
        // take refcell contents
        let cid = match *self.app_cid.lock().unwrap() {
            Some(cid) => cid,
            None => panic!("trying to set lba without cid being set!"),
        };

        let rw_ids = self.rw_ids.lock().unwrap();

        match xous::send_message(
            cid,
            xous::Message::new_blocking_scalar(rw_ids.max_lba_id, 0, 0, 0, 0),
        )
        .expect("cannot send message to block device app")
        {
            xous::Result::Scalar1(max_lba) => max_lba.try_into().unwrap(),
            _ => panic!("received response to max_lba that isn't a Scalar1!"),
        }
    }
}
