use bao1x_api::*;
#[cfg(feature = "usb")]
use usb_bao1x::UsbHid;
use xous::msg_scalar_unpack;

pub fn start_shell() {
    std::thread::spawn(move || {
        shell();
    });
}

////////////////// local message passing from Ux Callback
use num_traits::*;

use crate::repl::HISTORY_DEPTH;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum ConsoleOp {
    /// A character is incoming
    Keypress,
    /// exit the application
    Quit,
}

pub(crate) const SERVER_NAME_SHELLCHAT: &str = "_Bao console application_"; // used internally by xous-names

fn shell() {
    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let shch_sid = xns.register_name(SERVER_NAME_SHELLCHAT, None).expect("can't register server");

    let kbd = keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(SERVER_NAME_SHELLCHAT, ConsoleOp::Keypress.to_u32().unwrap() as usize);

    let mut repl = crate::repl::Repl::new(&xns);
    let mut update_repl = false;
    let mut was_callback = false;
    let mut history_index: isize = 0;

    #[cfg(feature = "usb")]
    let usb = UsbHid::new();

    let mut input = String::new();
    loop {
        let msg = xous::receive_message(shch_sid).unwrap();
        let console_op: Option<ConsoleOp> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", console_op);
        match console_op {
            Some(ConsoleOp::Keypress) => msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                #[cfg(feature = "usb")]
                usb.serial_send(&[k1 as u8]).ok();
                if k1 == 0x08 {
                    // backspace character
                    input.pop(); // returns None if empty
                } else if k == '↑' || k == '↓' {
                    // a partial attempt at history processing.
                    if k == '↑' {
                        history_index -= 1;
                    } else if k == '↓' {
                        history_index += 1;
                    }
                    if history_index > -1 {
                        history_index = -1;
                    }
                    if history_index < -(HISTORY_DEPTH as isize) {
                        history_index = -(HISTORY_DEPTH as isize);
                    }
                    let input_len = input.len();
                    // line editing doesn't work because the "print" implementation waits until a newline
                    // to send the buffer to reduce message traffic. I think it's reasonable to leave it
                    // without line editing for now to preserve this trade-off.
                    if false {
                        // clear the old input
                        let spaces: String = std::iter::repeat(' ').take(input_len).collect();
                        print!("\r{}", spaces);
                    }
                    input.clear();
                    if let Some(s) = repl.get_history(history_index) {
                        input.push_str(s);
                        // print the new input
                        println!("{}", &input);
                    }
                } else if k != '\u{0000}' && k != '\n' && k != '\r' {
                    input.push(k);
                } else if k == '\n' || k == '\r' {
                    repl.input(input.as_str()).expect("REPL crashed");
                    update_repl = true;
                    was_callback = false;
                    history_index = 0;
                    input.clear();
                }
            }),
            Some(ConsoleOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message, treating as callback");
                repl.msg(msg);
                update_repl = true;
                was_callback = true;
            }
        }
        if update_repl {
            repl.update(was_callback, true).expect("REPL had problems updating");
            update_repl = false;
        }
    }

    // clean up our program
    log::error!("shell loop exit, destroying servers");
    xns.unregister_server(shch_sid).unwrap();
    xous::destroy_server(shch_sid).unwrap();
}
