#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

use core::fmt::Write;

use rkyv::Unarchive;
use rkyv::archived_value;
use core::pin::Pin;

use ime_plugin_api::{ImeFrontEndApi, ImeFrontEnd};

use heapless::spsc::Queue;
use heapless::consts::U4;

struct Repl {
    input: Option<xous::String<1024>>,
    history: Queue<xous::String<1024>, U4>,
    gam: xous::CID,
}
impl Repl {
    fn new() -> Self {
        let gam_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("SHCH: can't connect to GAM");
        Repl {
            input: None,
            history: Queue::new(),
            gam: gam_conn,
        }
    }

    /// accept a new input string
    fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        let mut local = xous::String::<1024>::new();
        write!(local, "{}", line).expect("SHCH: line too long for history buffer");

        self.input = Some(local);

        Ok(())
    }

    /// update the loop, in response to various inputs
    fn update(&mut self) -> Result<(), xous::Error> {
        // if we had an input string, do something
        if let Some(local) = self.input {
            if self.history.len() == self.history.capacity() {
                self.history.dequeue().expect("SHCH: couldn't dequeue historye");
            }
            self.history.enqueue(local).expect("SHCH: couldn't store input line");
            self.input = None;

            for &s in self.history.iter() {
                info!("SHCH: command history {}", s);
            }
        }

        // other things to do based on other events...

        Ok(())
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = true;
    log_server::init_wait().unwrap();
    info!("SHCH: my PID is {}", xous::process::id());

    let shch_sid = xous_names::register_name(xous::names::SERVER_NAME_SHELLCHAT).expect("SHCH: can't register server");
    info!("SHCH: registered with NS -- {:?}", shch_sid);

    let imef_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_FRONT).expect("SHCH: can't connect to IMEF");
    let imef = ImeFrontEnd { connection: Some(imef_conn) };
    imef.register_listener(xous::names::SERVER_NAME_SHELLCHAT).expect("SHCH: couldn't request events from IMEF");

    let mut repl = Repl::new();
    let mut update_repl = false;
    info!("SHCH: starting main loop");
    loop {
        let envelope = xous::receive_message(shch_sid).unwrap();
        if debug1{info!("SHCH: got message {:?}", envelope);}
        if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<ime_plugin_api::ImefOpcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<ime_plugin_api::ImefOpcode>::GotInputLine(rkyv_s) => {
                    let s: xous::String<4000> = rkyv_s.unarchive();
                    repl.input(s.as_str().expect("SHCH: couldn't convert incoming string")).expect("SHCH: REPL couldn't accept input string");
                    update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                },
                _ => panic!("SHCH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else {
            error!("SHCH: couldn't convert message");
        }

        if update_repl {
            repl.update().expect("SHCH: REPL had problems updating");
            update_repl = false;
        }
    }
}
