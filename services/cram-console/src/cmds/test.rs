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
        let helpstring = "Test commands. See code for options.";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "ball" => {
                    let _ = std::thread::spawn(move || {
                        let xns = xous_names::XousNames::new().unwrap();
                        let mut ball = crate::ball::Ball::new(&xns);
                        log::info!("starting ball");
                        loop {
                            ball.update();
                        }
                    });
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
