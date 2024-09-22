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
        let helpstring = "ver [ec] [wf200] [soc] [dna] [xous] [ecreset]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "ec" => {
                    let (rev, dirty) = env.com.get_ec_git_rev().unwrap();
                    let dirtystr = if dirty { "dirty" } else { "clean" };
                    write!(ret, "EC gateware commit: {:x}, {}\n", rev, dirtystr).unwrap();
                    let ec_ver = env.com.get_ec_sw_tag().unwrap();
                    log::info!(
                        "{}VER.EC,{},{},{},{},{}",
                        xous::BOOKEND_START,
                        ec_ver.maj,
                        ec_ver.min,
                        ec_ver.rev,
                        ec_ver.extra,
                        xous::BOOKEND_END
                    );
                    write!(ret, "EC sw tag: {}", ec_ver.to_string()).unwrap();
                }
                "wf200" => {
                    let wf_ver = env.com.get_wf200_fw_rev().unwrap();
                    write!(ret, "Wf200 fw rev {}.{}.{}", wf_ver.maj, wf_ver.min, wf_ver.rev).unwrap();
                }
                "soc" => {
                    let soc_rev = env.llio.soc_gitrev().unwrap();
                    write!(ret, "SoC git rev {}", soc_rev.to_string()).unwrap();
                    log::info!(
                        "{}VER.SOC,{},{},{},{},{}",
                        xous::BOOKEND_START,
                        soc_rev.maj,
                        soc_rev.min,
                        soc_rev.rev,
                        soc_rev.extra,
                        xous::BOOKEND_END
                    );
                }
                "dna" => {
                    write!(ret, "SoC silicon DNA: 0x{:x}", env.llio.soc_dna().unwrap()).unwrap();
                }
                "xous" => {
                    write!(ret, "Xous version: {}", env.ticktimer.get_version()).unwrap();
                    log::info!(
                        "{}VER.XOUS,{},{}",
                        xous::BOOKEND_START,
                        env.ticktimer.get_version(),
                        xous::BOOKEND_END
                    );
                }
                "ecreset" => {
                    env.llio.ec_reset().unwrap();
                    env.ticktimer.sleep_ms(4000).unwrap();
                    env.com.link_reset().unwrap();
                    env.com.reseed_ec_trng().unwrap();
                    write!(ret, "EC has been reset, and new firmware loaded.").unwrap();
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
