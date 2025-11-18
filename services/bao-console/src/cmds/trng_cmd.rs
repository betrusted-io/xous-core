use String;
use rand_core::RngCore;

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
        let helpstring = "trng [pump] [u32] [u64]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                // used to test reseeding of the TRNG API
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
                "u32" => {
                    log::info!("A u32 trng value: {:x}", env.trng.get_u32().unwrap());
                }
                "u64" => {
                    log::info!("A u64 trng value: {:x}", env.trng.get_u64().unwrap());
                }
                "buf" => {
                    let mut test1 = [0u8; 8192];
                    let mut trng = bao1x_hal_service::trng::Trng::new(&env.xns).unwrap();
                    trng.fill_bytes(&mut test1);
                    log::info!("8192 case");
                    for chunk in test1.chunks(32) {
                        for word in chunk.chunks(4) {
                            if word == &[0, 0, 0, 0] {
                                log::info!("zeroes found!");
                            }
                        }
                        log::info!("{:02x?}", chunk);
                    }
                    log::info!("4096 case");
                    let mut test2 = [0u8; 4096];
                    trng.fill_bytes(&mut test2);
                    for chunk in test2.chunks(32) {
                        for word in chunk.chunks(4) {
                            if word == &[0, 0, 0, 0] {
                                log::info!("zeroes found!");
                            }
                        }
                        log::info!("{:02x?}", chunk);
                    }
                    log::info!("4597 case"); // this is a prime number so it doesn't divide into anything
                    let mut test3 = [0u8; 4597];
                    trng.fill_bytes(&mut test3);
                    for chunk in test3.chunks(32) {
                        for word in chunk.chunks(4) {
                            if word == &[0, 0, 0, 0] {
                                log::info!("zeroes found!");
                            }
                        }
                        log::info!("{:02x?}", chunk);
                    }
                    log::info!("125 case");
                    let mut test4 = [0u8; 125];
                    trng.fill_bytes(&mut test4);
                    for chunk in test4.chunks(32) {
                        for word in chunk.chunks(4) {
                            if word == &[0, 0, 0, 0] {
                                log::info!("zeroes found!");
                            }
                        }
                        log::info!("{:02x?}", chunk);
                    }
                    log::info!("61 case");
                    let mut test5 = [0u8; 61];
                    trng.fill_bytes(&mut test5);
                    for chunk in test4.chunks(32) {
                        for word in chunk.chunks(4) {
                            if word == &[0, 0, 0, 0] {
                                log::info!("zeroes found!");
                            }
                        }
                        log::info!("{:02x?}", chunk);
                    }
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
