use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct TrngCmd {}
impl TrngCmd {
    pub fn new() -> Self { TrngCmd {} }
}

impl<'a> ShellCmdApi<'a> for TrngCmd {
    cmd_api!(trng);

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "trng [pump]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "pump" => {
                    const ROUNDS: usize = 16;
                    for i in 0..ROUNDS {
                        log::debug!("pump round {}", i);
                        let mut buf: [u32; 1024] = [0; 1024];
                        env.trng.fill_buf(&mut buf).unwrap();
                        log::debug!("pump samples: {:x}, {:x}, {:x}", buf[0], buf[512], buf[1023]);
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
