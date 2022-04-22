use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct Ver {
}


impl<'a> ShellCmdApi<'a> for Ver {
    cmd_api!(ver); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "ver options: ec, wf200, soc, dna, xous";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "ec" => {
                    let (rev, dirty) = env.com.get_ec_git_rev().unwrap();
                    let dirtystr = if dirty { "dirty" } else { "clean" };
                    write!(ret, "EC gateware commit: {:x}, {}\n", rev, dirtystr).unwrap();
                    let (maj, min, rev, commit) = env.com.get_ec_sw_tag().unwrap();
                    log::info!("EC sw tag: {}.{}.{}+{}", maj, min, rev, commit);
                    write!(ret, "EC sw tag: {}.{}.{}+{}", maj, min, rev, commit).unwrap();
                }
                "wf200" => {
                    let (maj, min, rev) = env.com.get_wf200_fw_rev().unwrap();
                    write!(ret, "Wf200 fw rev {}.{}.{}", maj, min, rev).unwrap();
                }
                "soc" => {
                    let (maj, min, rev, extra, gitrev) = env.llio.soc_gitrev().unwrap();
                    write!(ret, "SoC git rev {}.{}.{}+{}, commit {:x}", maj, min, rev, extra, gitrev).unwrap();
                }
                "dna" => {
                    write!(ret, "SoC silicon DNA: 0x{:x}", env.llio.soc_dna().unwrap()).unwrap();
                }
                "xous" => {
                    write!(ret, "Xous version: {}", env.ticktimer.get_version()).unwrap();
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
