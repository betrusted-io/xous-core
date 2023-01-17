use crate::{ShellCmdApi,CommonEnv,heap_usage};
use xous_ipc::String as XousString;
use core::fmt::Write;

#[derive(Debug)]
pub struct Heap {
}
impl Heap {
    pub fn new() -> Self {
        Heap {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Heap {
    cmd_api!(heap);

    fn process(&mut self, _args: XousString::<1024>, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        let heap = heap_usage();
        write!(ret, "heap usage: {}", heap).unwrap();
        log::info!("heap usage: {}", heap);
        Ok(Some(ret))
    }
}
