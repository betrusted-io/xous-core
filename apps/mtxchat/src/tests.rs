use chat::Chat;

pub fn test_ui(chat: &Chat) {
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let pddb = pddb::Pddb::new();
    // nuke any existing test dictionary, if it exists
    pddb.delete_dict("tests.ui", None).ok();
    pddb.sync().ok();

    // re-create the test room from scratch every time
    chat.dialogue_set("tests.ui", Some("test_room")).expect("couldn't set dialog");

    // generate a list of test posts that are repeatable each time
    chat.post_add("alice", 1_700_000_000, "hello world!", None).ok();
    chat.post_add("bob", 1_700_000_002, "hi alice!", None).ok();

    tt.sleep_ms(2000).ok();
    log::info!("triggering save");
    xous::send_message(
        chat.cid(),
        xous::Message::new_scalar(chat::ChatOp::DialogueSave as usize, 0, 0, 0, 0),
    )
    .expect("failed to send new inbound msgs");

    log::info!("sleep");
    tt.sleep_ms(5000).ok();
    log::info!("redraw");
    chat.redraw();

    loop {
        tt.sleep_ms(5000).ok();
        log::info!("test done, displaying result");
    }
}
