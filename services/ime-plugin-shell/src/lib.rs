#![cfg_attr(target_os = "none", no_std)]

use ime_plugin_api::*;

use xous::{CID, String};

pub fn get_prediction_triggers(cid: CID) -> Result<PredictionTriggers, xous::Error> {
    let response = xous::send_message(cid, Opcode::GetPredictionTriggers.into())?;
    if let xous::Result::Scalar1(code) = response {
        Ok(code.into())
    } else {
        panic!("get_prediciton_triggers failed")
    }
}

pub fn unpick(cid: CID) -> Result<(), xous::Error> {
    xous::send_message(cid, Opcode::Unpick.into())?;
    Ok(())
}

pub fn set_input(cid: CID, s: String<4096>) -> Result<(), xous::Error> {
    use rkyv::Write;

    let rkyv_input = Opcode::Input(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_input).expect("IMES|API: couldn't archive input string");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("IMES|API: set_input operation failure");

    Ok(())
}

pub fn feedback_picked(cid: CID, s: String<4096>) -> Result<(), xous::Error> {
    use rkyv::Write;

    let rkyv_picked = Opcode::Picked(s);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_picked).expect("IMES|API: couldn't archive picked string");
    let xous_buffer = writer.into_inner();

    xous_buffer.lend(cid, pos as u32).expect("IMES|API: feedback_picked operation failure");

    Ok(())
}

pub fn get_prediction(cid: CID, index: u32) -> Result<xous::String<4096>, xous::Error> {
    use rkyv::Write;
    use rkyv::Unarchive;

    let prediction = Prediction {
        index,
        string: xous::String::<4096>::new(),
    };
    let pred_op = Opcode::Prediction(prediction);

    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&pred_op).expect("IMES|API: couldn't archive prediction request");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(cid, pos as u32).expect("IMES|API: prediction fetch operation failure");

    let returned = unsafe { rkyv::archived_value::<Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<Opcode>::Prediction(result) = returned {
        let pred_r: Prediction = result.unarchive();

        let retstring: xous::String<4096> = pred_r.string.clone();
        Ok(retstring)
    } else {
        let r = returned.unarchive();
        log::info!("get_prediciton saw an unhandled return of {:?}", r);
        Err(xous::Error::InvalidString)
    }
}

