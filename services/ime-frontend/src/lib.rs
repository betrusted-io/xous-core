#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};

pub fn set_input_canvas(cid: CID, g: graphics_server::Gid) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetInputCanvas(g).into())?;
    Ok(())
}

pub fn set_prediction_canvas(cid: CID, g: graphics_server::Gid) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetPredictionCanvas(g).into())?;
    Ok(())
}

pub fn set_predictor(cid: CID, servername: &str) -> Result<(), xous::Error> {
    let mut server = xous::String::<256>::new();
    use core::fmt::Write;
    write!(server, "{}", servername).expect("IMEF: couldn't write set_predictor server name");
    let ime_op = api::Opcode::SetPredictionServer(server);

    use rkyv::Write as ArchiveWrite;
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&ime_op).expect("IMEF: couldn't archive SetPredictionServer");
    writer.into_inner().lend(cid, pos as u32).expect("IMEF: SetPredicitonServer request failure");
    Ok(())
}