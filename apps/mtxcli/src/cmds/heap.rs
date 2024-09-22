use core::fmt::Write;



use crate::{heap_usage, CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Heap {}
impl Heap {
    pub fn new() -> Self { Heap {} }
}

impl<'a> ShellCmdApi<'a> for Heap {
    cmd_api!(heap);

    fn process(&mut self, _args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let heap = heap_usage();
        write!(ret, "heap usage: {}", heap).unwrap();
        log::info!("heap usage: {}", heap);
        Ok(Some(ret))
    }
}
