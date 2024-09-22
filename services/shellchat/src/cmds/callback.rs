use core::sync::atomic::{AtomicBool, Ordering};

use xous::{Message, MessageEnvelope, ScalarMessage};
use String;

use crate::{CommonEnv, ShellCmdApi};
static CB_RUN: AtomicBool = AtomicBool::new(false);
pub fn callback_thread() {
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let xns = xous_names::XousNames::new().unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

    log::info!("callback initiator test thread started");
    loop {
        if CB_RUN.load(Ordering::Relaxed) {
            CB_RUN.store(false, Ordering::Relaxed);
            ticktimer.sleep_ms(10000).unwrap();
            // just send a bogus message
            xous::send_message(
                callback_conn,
                Message::Scalar(ScalarMessage { id: 0xdeadbeef, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            )
            .unwrap();
        } else {
            ticktimer.sleep_ms(250).unwrap(); // a little more polite than simply busy-waiting
        }
    }
}
#[derive(Debug)]
pub struct CallBack {
    state: u32,
    callbacks: u32,
}
impl CallBack {
    pub fn new() -> Self {
        xous::create_thread_0(callback_thread).expect("SHCH: couldn't create callback generator thread");
        CallBack { state: 0, callbacks: 0 }
    }
}

impl<'a> ShellCmdApi<'a> for CallBack {
    cmd_api!(cb);

    fn process(&mut self, _args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;

        self.state += 1;
        let mut ret = String::new();
        write!(ret, "CallBack has initiated {} times.", self.state).unwrap();
        CB_RUN.store(true, Ordering::Relaxed);
        Ok(Some(ret))
    }

    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;

        self.callbacks += 1;
        let mut ret = String::new();
        write!(ret, "CallBack #{}, with data {:?}", self.state, msg).unwrap();
        Ok(Some(ret))
    }
}
