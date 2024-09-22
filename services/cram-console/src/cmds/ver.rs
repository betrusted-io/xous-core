use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Ver {}

impl<'a> ShellCmdApi<'a> for Ver {
    cmd_api!(ver);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "ver [xous]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "xous" => {
                    write!(ret, "Xous version: {}", env.ticktimer.get_version()).unwrap();
                    log::info!(
                        "{}VER.XOUS,{},{}",
                        xous::BOOKEND_START,
                        env.ticktimer.get_version(),
                        xous::BOOKEND_END
                    );
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
