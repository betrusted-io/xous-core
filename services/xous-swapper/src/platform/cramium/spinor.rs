use core::sync::atomic::{AtomicBool, Ordering};

use cramium_api::SpimChannel;
use cramium_hal::udma::*;
use xous_ipc::Buffer;

use crate::api::{BulkErase, Opcode, WriteRegion};

static OP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn spinor_handler() -> ! {
    let channel = SpimChannel::Channel1;

    // safety: this is safe because the global clocks were gated on by the bootloader
    let flash_spim = unsafe {
        Spim::new(
            channel,
            25_000_000,
            50_000_000,
            SpimClkPol::LeadingEdgeRise,
            SpimClkPha::CaptureOnLeading,
            SpimCs::Cs0,
            0,
            0,
            None,
            1024, // this is limited by the page length
            1024,
            Some(6),
            Some(SpimMode::Quad),
        )
    };

    let xns = xous_api_names::XousNames::new().unwrap();
    let sid = xns.register_name(crate::api::SERVER_NAME_SPINOR, None).unwrap();
    let mut client_id: Option<[u32; 4]> = None;

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::Invalid);
        log::debug!("{:?}", opcode);
        match opcode {
            Opcode::AcquireExclusive => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let id0 = scalar.arg1;
                    let id1 = scalar.arg2;
                    let id2 = scalar.arg3;
                    let id3 = scalar.arg4;
                    if client_id.is_none() {
                        OP_IN_PROGRESS.store(true, Ordering::Relaxed); // lock out suspends when the exclusive lock is acquired
                        client_id = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                        log::trace!("giving {:x?} an exclusive lock", client_id);
                        scalar.arg1 = 1;
                    } else {
                        scalar.arg1 = 0;
                    }
                }
            }
            Opcode::ReleaseExclusive => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    client_id = None;
                    OP_IN_PROGRESS.store(false, Ordering::Relaxed);
                    scalar.arg1 = 1;
                }
            }
            Opcode::WriteRegion => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut wr = buffer.to_original::<WriteRegion, _>().unwrap();
                // TODO: implement this. `spinor` would be created in this implementation somewhere.
                // note: this must reject out-of-bound length requests for security reasons
                // wr.result = Some(spinor.write_region(&mut wr));
                todo!("implement `spinor`");

                buffer.replace(wr).expect("couldn't return response code to WriteRegion");
            }
            Opcode::BulkErase => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut wr = buffer.to_original::<BulkErase, _>().unwrap();
                // TODO: implement this. `spinor` would be created in this implementation somewhere.
                // note: this must reject out-of-bound length requests for security reasons
                // wr.result = Some(spinor.bulk_erase(&mut wr));
                todo!("implement `spinor`");

                buffer.replace(wr).expect("couldn't return response code to WriteRegion");
            }
            Opcode::Invalid => {
                log::error!("Invalid Opcode!");
            }
        }
    }
}
