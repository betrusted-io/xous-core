use std::sync::Arc;

use chat::ChatOp;
use locales::t;
use tls::xtls::TlsConnector;
use url::Url;
use xous::CID;
use xous_ipc::Buffer;

use crate::{MTX_LONG_TIMEOUT_MS, get_username, web};

pub fn listen(
    url: &mut Url,
    token: &str,
    room_id: &str,
    since: Option<&str>,
    filter: &str,
    dialogue_id: &str,
    chat_cid: CID,
) {
    log::info!("client_sync for {} ms...", MTX_LONG_TIMEOUT_MS);

    let mut agent = ureq::builder().tls_connector(Arc::new(TlsConnector {})).build();
    if let Some((_since, events)) =
        web::client_sync(url, filter, since, MTX_LONG_TIMEOUT_MS, &room_id, &token, &mut agent)
    {
        // TODO utilize "since"
        // and you probably want to have a look at Dialogue::MAX_BYTES

        // TODO resolve suspected race condition
        // This progress modal is masking a bug by slowing the loop down
        // Precursor "Guru Mediation" `voilated: nonNull::new_unchecked`
        chat::cf_set_status_text(chat_cid, t!("mtxchat.busy.rx_events", locales::LANG));
        chat::cf_set_busy_state(chat_cid, true);
        let mut event_count = 0;
        for event in events {
            let sender = event.sender.unwrap_or("anon".to_string());
            let body = event.body.unwrap_or("...".to_string());
            let post = chat::Post {
                dialogue_id: String::from(dialogue_id),
                author: String::from(&get_username(&sender)),
                timestamp: event.ts.unwrap_or(0),
                text: String::from(&body),
                attach_url: None,
            };
            match Buffer::into_buf(post) {
                Ok(buf) => buf.send(chat_cid, ChatOp::PostAdd as u32).map(|_| ()),
                Err(_) => Err(xous::Error::InternalError),
            }
            .expect("failed to convert post into buffer");
            event_count += 1;
            chat::cf_set_status_text(
                chat_cid,
                &format!("{} {}", t!("mtxchat.busy.rx_events", locales::LANG), event_count),
            );
        }
    }
    chat::cf_set_busy_state(chat_cid, false);
    // trigger the chat ui to save the dialogue to the pddb
    xous::send_message(chat_cid, xous::Message::new_scalar(ChatOp::DialogueSave as usize, 0, 0, 0, 0))
        .expect("failed to send new inbound msgs");
}
