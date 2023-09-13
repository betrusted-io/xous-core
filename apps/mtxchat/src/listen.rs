use crate::{get_username, web, MTX_LONG_TIMEOUT};
use chat::ChatOp;
use modals::Modals;
use std::sync::Arc;
use tls::Tls;
use xous::CID;
use xous_ipc::Buffer;

pub fn listen(
    server: &str,
    token: &str,
    room_id: &str,
    since: Option<&str>,
    filter: &str,
    chat_cid: CID,
) {
    let xns = xous_names::XousNames::new().unwrap();
    let modals = Modals::new(&xns).expect("can't connect to Modals server");
    log::info!("client_sync for {} ms...", MTX_LONG_TIMEOUT);

    let tls = Tls::new();
    let mut agent = ureq::builder()
        .tls_config(Arc::new(tls.client_config()))
        .build();
    if let Some((_since, events)) = web::client_sync(
        server,
        filter,
        since,
        MTX_LONG_TIMEOUT,
        &room_id,
        &token,
        &mut agent,
    ) {
        // TODO utilize "since"
        // and you probably want to have a look at Dialogue::MAX_BYTES

        // TODO resolve suspected race condition
        // This progress modal is masking a bug by slowing the loop down
        // Precursor "Guru Mediation" `voilated: nonNull::new_unchecked`
        modals
            .start_progress("Receiving events ...", 0, events.len() as u32, 0)
            .expect("no progress bar");
        let mut event_count = 0;
        for event in events {
            let sender = event.sender.unwrap_or("anon".to_string());
            let body = event.body.unwrap_or("...".to_string());
            let post = chat::Post {
                author: xous_ipc::String::from_str(&get_username(&sender)),
                timestamp: event.ts.unwrap_or(0),
                text: xous_ipc::String::from_str(&body),
                attach_url: None,
            };
            match Buffer::into_buf(post) {
                Ok(buf) => buf.send(chat_cid, ChatOp::PostAdd as u32).map(|_| ()),
                Err(_) => Err(xous::Error::InternalError),
            }
            .expect("failed to convert post into buffer");
            event_count += 1;
            modals
                .update_progress(event_count)
                .expect("no progress update");
        }
    }
    modals.finish_progress().expect("failed progress finish");
    // trigger the chat ui to save the dialogue to the pddb
    xous::send_message(
        chat_cid,
        xous::Message::new_scalar(ChatOp::DialogueSave as usize, 0, 0, 0, 0),
    )
    .expect("failed to send new inbound msgs");
}
