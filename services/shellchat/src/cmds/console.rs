use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Console {}

impl<'a> ShellCmdApi<'a> for Console {
    cmd_api!(console);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "Serial console options: kernel, log, app";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "kernel" => {
                    env.llio.set_uart_mux(llio::UartType::Kernel).unwrap();
                    write!(ret, "kernel -> serial console").unwrap();
                }
                "log" => {
                    env.llio.set_uart_mux(llio::UartType::Log).unwrap();
                    write!(ret, "log -> serial console").unwrap();
                }
                "app" => {
                    env.llio.set_uart_mux(llio::UartType::Application).unwrap();
                    write!(ret, "app -> serial console").unwrap();
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
