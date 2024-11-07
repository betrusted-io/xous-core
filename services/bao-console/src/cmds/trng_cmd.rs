use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct TrngCmd {}
impl TrngCmd {
    pub fn new() -> Self { TrngCmd {} }
}

impl<'a> ShellCmdApi<'a> for TrngCmd {
    cmd_api!(trng);

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "trng [pump]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "pump" => {
                    const ROUNDS: usize = 16;
                    for i in 0..ROUNDS {
                        log::info!("pump round {}", i);
                        let mut buf: [u32; 1020] = [0; 1020];
                        env.trng.fill_buf(&mut buf).unwrap();
                        log::info!("pump samples: {:x}, {:x}, {:x}", buf[0], buf[512], buf[1019]);
                    }
                    write!(ret, "Pumped {}x1k values out of the engine", ROUNDS).unwrap();
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
