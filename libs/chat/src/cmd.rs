use crate::{now, Chat};

use locales::t;
use std::convert::TryInto;
use std::thread;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum TestOp {
    Post = 0,
    Event,
    RawKeys,
}

pub fn shellchat<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<Option<String>, xous::Error> {
    use core::fmt::Write;
    let mut ret = String::new();
    match tokens.next() {
        Some("read") => {
            if let Some(dict) = tokens.next() {
                if let Some(key) = tokens.next() {
                    let _chat = Chat::read_only(dict, key);
                }
            }
        }
        Some("test") => {
            log::info!("starting chat test command");

            let xns = xous_names::XousNames::new().unwrap();
            let sid = xns
                .register_name("Chat test", None)
                .expect("can't register server");
            log::trace!("registered with NS -- {:?}", sid);

            enum TestOp {
                Event,
                Post,
                RawKeys,
            }

            let chat = Chat::new(
                Some(xous::connect(sid).unwrap()),
                Some(TestOp::Post as usize),
                Some(TestOp::Event as usize),
                Some(TestOp::RawKeys as usize),
            );

            chat.dialogue_set("chat_test", "test");
            chat.post_add("system", now(), "chat test commenced", None);

            chat.post_add("system", now(), "chat test concluded", None);
            log::info!("finished chat test command");
        }
        // helpful stuff
        Some("help") => {
            write!(ret, "{}", t!("tls.cmd_help", locales::LANG)).ok();
        }
        None | _ => {
            write!(ret, "{}\n", t!("chat.cmd", locales::LANG)).ok();
            write!(ret, "\ttest\t{}\n", t!("tls.test_cmd", locales::LANG)).ok();
            write!(ret, "\thelp\n").ok();
        }
    }
    Ok(Some(ret))
}
