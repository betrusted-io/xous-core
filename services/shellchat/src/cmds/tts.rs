use core::fmt::Write;

use tts_frontend::*;
use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Tts {
    pub fe: TtsFrontend,
}
impl Tts {
    pub fn new(xns: &xous_names::XousNames) -> Tts { Tts { fe: TtsFrontend::new(xns).unwrap() } }
}

impl<'a> ShellCmdApi<'a> for Tts {
    cmd_api!(tts);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "tts options: speak";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "speak" => {
                    let mut text = String::<1024>::new();
                    join_tokens(&mut text, &mut tokens);
                    self.fe.tts_simple(text.as_str().expect("not valid utf-8")).unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

fn join_tokens<'a>(buf: &mut String<1024>, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
