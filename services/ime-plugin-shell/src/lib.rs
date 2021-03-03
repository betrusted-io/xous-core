#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, String};

pub fn set_input(cid: CID, s: String<4096>) -> Result<(), xous::Error> {
    use rkyv::Write;

    let rkyv_input = api::Opcode::Input(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_input).expect("IMES|API: couldn't archive input string");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("IMES|API: set_input operation failure");

    Ok(())
}

pub fn feedback_picked(cid: CID, s: String<4096>) -> Result<(), xous::Error> {
    use rkyv::Write;

    let rkyv_picked = api::Opcode::Picked(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_picked).expect("IMES|API: couldn't archive picked string");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("IMES|API: feedback_picked operation failure");

    Ok(())
}

pub fn get_prediction(cid: CID, index: u32) -> Result<xous::String<4096>, xous::Error> {
    use rkyv::Write;
    use rkyv::Unarchive;

    let prediction = api::Prediction {
        index,
        string: xous::String::<4096>::new(),
    };
    let pred_op = api::Opcode::Prediction(prediction);

    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&pred_op).expect("IMES|API: couldn't archive prediction request");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(cid, pos as u32).expect("IMES|API: prediction fetch operation failure");

    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::Prediction(result) = returned {
        let pred_r: api::Prediction = result.unarchive();

        let retstring: xous::String<4096> = pred_r.string.clone();
        Ok(retstring)
    } else {
        let r = returned.unarchive();
        log::info!("get_prediciton saw an unhandled return of {:?}", r);
        Err(xous::Error::InvalidString)
    }
}