#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod ball;
use ball::*;
use num_traits::*;
use xous::Message;

// This name should be (1) unique (2) under 64 characters long and (3) ideally descriptive.
const BALL_SERVER_NAME: &'static str = "User app 'ball'";

/// Opcodes for the application main loop
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum AppOp {
    /// pumps the state of the ball
    Pump,
    /// redraw our screen
    Redraw,
    /// handle raw key input
    Rawkeys,
    /// handle focus change
    FocusChange,
    /// exit the application
    Quit,
}

const BALL_UPDATE_RATE_MS: usize = 50;

/// Opcodes from the Pump thread loop
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum PumpOp {
    Run,
    Stop,
    Pump,
    Quit,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(BALL_SERVER_NAME, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", sid);

    // create the ball object
    let mut ball = Ball::new(sid);

    // build a thread that will send periodic pumps to the main thread when the system is told to run,
    // but also not busy-wait when it is told to stop
    let pump_sid = xous::create_server().unwrap();
    let cid_to_pump = xous::connect(pump_sid).unwrap();
    ball::ball_pump_thread(xous::connect(sid).unwrap(), pump_sid);

    // this is the main event loop for the app.
    let mut allow_redraw = true;
    let mut into_allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(AppOp::Redraw) => {
                if allow_redraw {
                    ball.focus();
                    ball.update();
                }
            }
            Some(AppOp::Pump) => {
                // this is a blocking scalar so that pump calls don't initiate faster than the app can draw
                if into_allow_redraw {
                    ball.focus();
                    into_allow_redraw = false;
                }
                if allow_redraw {
                    ball.update();
                }
                xous::return_scalar(msg.sender, 1).expect("couldn't ack pump");
            }
            Some(AppOp::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                ball.rawkeys(keys);
            }),
            Some(AppOp::FocusChange) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                log::info!("focus change: {:?}", new_state);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false; // this instantly terminates future updates, even if Pump messages are in our input queue
                        xous::send_message(
                            cid_to_pump,
                            Message::new_scalar(PumpOp::Stop.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't send stop message to the pump thread");
                    }
                    gam::FocusState::Foreground => {
                        into_allow_redraw = true;
                        allow_redraw = true;
                        xous::send_message(
                            cid_to_pump,
                            Message::new_scalar(PumpOp::Run.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't send run message to the pump thread");
                    }
                }
            }),
            Some(AppOp::Quit) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::send_message(
                    cid_to_pump,
                    Message::new_blocking_scalar(PumpOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't send run message to the pump thread");
                unsafe { xous::disconnect(cid_to_pump).ok() };
                xous::return_scalar(msg.sender, 1).expect("couldn't acknowledge quit message");
                break;
            }),
            _ => log::error!("couldn't convert opcode: {:?}", msg),
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
