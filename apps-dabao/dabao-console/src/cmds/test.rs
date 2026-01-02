use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Test {}

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "Test commands. See code for options.";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "timer" => {
                    let start = _env.ticktimer.elapsed_ms();
                    log::info!("Starting test");
                    let mut seconds = 0;
                    loop {
                        let elapsed = _env.ticktimer.elapsed_ms() - start;
                        if elapsed > seconds * 1000 {
                            log::info!("{} s", seconds);
                            seconds += 1;
                        }
                    }
                }
                "env" => {
                    log::info!("{:?}", std::env::vars());
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
