use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

pub struct PddbCmd {
    pddb: pddb::Pddb,
}
impl PddbCmd {
    pub fn new(_xns: &xous_names::XousNames) -> PddbCmd {
        PddbCmd {
            pddb: pddb::Pddb::new(),
        }
    }
}

impl<'a> ShellCmdApi<'a> for PddbCmd {
    cmd_api!(pddb); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "pddb [basislist]";

        let mut tokens = args.as_str().unwrap().split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "basislist" => {
                    let bases = self.pddb.list_basis();
                    for basis in bases {
                        write!(ret, "{}\n", basis).unwrap();
                    }
                    /* // example of using .get with a callback
                    self.pddb.get("foo", "bar", None, false, false,
                        Some({
                            let cid = cid.clone();
                            let counter = self.counter.clone();
                            move || {
                            xous::send_message(cid, xous::Message::new_scalar(0, counter as usize, 0, 0, 0)).expect("couldn't send");
                        }})
                    ).unwrap();*/
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
