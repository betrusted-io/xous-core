use early_settings::{Opcode, SERVER_NAME_ES};
use num_traits::FromPrimitive;
use spinor::Spinor;
use xous::{msg_blocking_scalar_unpack, MemoryRange};

struct State {
    settings_page: MemoryRange,
    spinor: Spinor,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    let state = State {
        settings_page: xous::syscall::map_memory(
            xous::MemoryAddress::new((xous::EARLY_SETTINGS + xous::FLASH_PHYS_BASE) as usize),
            None,
            4096,
            xous::MemoryFlags::R,
        )
        .unwrap(),
        spinor: spinor::Spinor::new(&xns).unwrap(),
    };

    let sid = xns
        .register_name(SERVER_NAME_ES, None)
        .expect("can't register server");

    loop {
        let msg = xous::receive_message(sid).unwrap(); // this blocks until we get a message
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::GetKeymap) => {
                let settings: &[u8] = state.settings_page.as_slice();

                let code = u32::from_le_bytes(settings[..4].try_into().unwrap());

                xous::return_scalar(msg.sender, code as usize).unwrap();
            }
            Some(Opcode::SetKeymap) => msg_blocking_scalar_unpack!(msg, map, _, _, _, {
                let code = (map as u32).to_le_bytes();
                let settings: &[u8] = state.settings_page.as_slice();

                state
                    .spinor
                    .patch(settings, xous::EARLY_SETTINGS, &code, 0)
                    .expect("couldn't patch our keyboard code");

                log::info!("writing early keymap: {}", map);

                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::SetEarlySleep) => msg_blocking_scalar_unpack!(msg, value, _, _, _, {
                let code = (value as u32).to_le_bytes();
                let settings: &[u8] = state.settings_page.as_slice();

                state
                    .spinor
                    .patch(settings, xous::EARLY_SETTINGS, &code, 4)
                    .expect("couldn't patch early reboot flag");

                log::info!("writing must sleep on reboot: {}", value);

                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::EarlySleep) => {
                let settings: &[u8] = state.settings_page.as_slice();

                let value = u32::from_le_bytes(settings[4..8].try_into().unwrap());

                log::info!("value read for early sleep: {}", value);

                xous::return_scalar(msg.sender, value as usize).unwrap();
            }
            _ => log::warn!("unrecognized opcode"),
        }
    }
}
