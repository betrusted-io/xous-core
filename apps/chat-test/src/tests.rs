use chat::Chat;

pub fn test_ui(chat: &Chat) {
    let pddb = pddb::Pddb::new();
    // nuke any existing test dictionary, if it exists
    pddb.delete_dict("tests.ui", None).ok();
    pddb.sync().ok();

    // re-create the test room from scratch every time
    chat.dialogue_set("tests.ui", Some("test_room")).expect("couldn't set dialog");

    // generate a list of test posts that are repeatable each time
    chat.post_add("alice", 1_700_000_000, "hello world!", None).ok();
    chat.post_add("bob", 1_700_000_002, "hi alice!", None).ok();
    for i in 0..5 {
        chat.post_add("alice", 1_700_000_005 + i*4, &format!("alice sez {}", i), None).ok();
        chat.post_add("bob", 1_700_000_006 + i*4, &format!("bob sez {}", i), None).ok();
        chat.post_add("trent", 1_700_000_007 + i*4, &format!("trent sez {}", i), None).ok();
    }
    chat.post_add("alice", 1_700_001_000, "eom", None).ok();

    log::info!("triggering save");
    xous::send_message(
        chat.cid(),
        xous::Message::new_scalar(chat::ChatOp::DialogueSave as usize, 0, 0, 0, 0),
    )
    .expect("failed to send new inbound msgs");

    log::info!("test setup done");
}
