use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct JtagCmd {
    jtag: jtag::Jtag,
}
impl JtagCmd {
    pub fn new(xns: &xous_names::XousNames) -> JtagCmd {
        JtagCmd { jtag: jtag::Jtag::new(&xns).expect("couldn't connect to JTAG block") }
    }
}

impl<'a> ShellCmdApi<'a> for JtagCmd {
    cmd_api!(jtag);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "jtag [id] [dna] [efuse] [cntl] [reset] [burn0]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "id" => {
                    let id = self.jtag.get_id().unwrap();
                    write!(ret, "JTAG idcode: 0x{:x}", id).unwrap();
                }
                "dna" => {
                    let dna = self.jtag.get_dna().unwrap();
                    write!(ret, "JTAG idcode: 0x{:x}", dna).unwrap();
                }
                "efuse" => {
                    let efuse = self.jtag.efuse_fetch().unwrap();
                    write!(
                        ret,
                        "User: 0x{:x}\nCntl: 0x{:x}\n,Fuse: {:x?}",
                        efuse.user, efuse.cntl, efuse.key
                    )
                    .unwrap();
                }
                "ir" => {
                    if let Some(val) = tokens.next() {
                        let intval = u8::from_str_radix(val, 2).unwrap();
                        self.jtag.write_ir(intval).unwrap();
                        write!(ret, "sending IR of 0x{:x}", intval).unwrap();
                    } else {
                        write!(ret, "ir needs an argument!").unwrap();
                    }
                }
                "burn0" => match self.jtag.efuse_key_burn([0; 32]) {
                    Ok(res) => {
                        if res {
                            write!(ret, "efuse key dummy burn was successful").unwrap();
                        } else {
                            write!(ret, "efuse key dummy burn was a failure").unwrap();
                        }
                    }
                    Err(e) => {
                        write!(ret, "internal error in doing efuse dummy key burn: {:?}", e).unwrap();
                    }
                },
                /* // for testing sealing only -- does not make sense for any normal context, but essential for debugging efuse issues.
                "seal" => {
                    use locales::t;
                    log::info!("{}EFUSE.SEAL,{}", precursor_hal::board::BOOKEND_START, precursor_hal::board::BOOKEND_END);
                    match self.jtag.seal_device() {
                        Ok(result) => {
                            if !result {
                                log::info!("{}", t!("rootkeys.efuse_seal_fail", locales::LANG));
                            } else {
                                log::info!("eFuse sealing success!");
                            }
                        }
                        Err(e) => {
                            log::info!("{}", &format!("{}\n{:?}", t!("rootkeys.efuse_internal_error", locales::LANG), e));
                        }
                    }
                    log::info!("{}EFUSE.SEAL_OK,{}", precursor_hal::board::BOOKEND_START, precursor_hal::board::BOOKEND_END);
                }
                */
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
