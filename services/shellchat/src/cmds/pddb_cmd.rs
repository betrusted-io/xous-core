use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

pub struct PddbCmd {
    manager: pddb::PddbBasisManager,
}
impl PddbCmd {
    pub fn new(xns: &xous_names::XousNames) -> PddbCmd {
        PddbCmd {
            manager: pddb::PddbBasisManager::new(),
        }
    }
}

impl<'a> ShellCmdApi<'a> for PddbCmd {
    cmd_api!(pddb); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "pddb [basislist]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "basislist" => {
                    let bases = self.manager.list_basis();
                    for basis in bases {
                        write!(ret, "{}\n", basis).unwrap();
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
