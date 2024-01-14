use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Accel {}

impl<'a> ShellCmdApi<'a> for Accel {
    cmd_api!(accel);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "accel has no options";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                _ => {
                    let (x, y, z, id) = env.com.gyro_read_blocking().unwrap();
                    write!(ret, "x: {} y: {} z: {}, id: 0x{:x}", x, y, z, id).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
