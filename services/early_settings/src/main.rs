use std::ops::Range;

use early_settings::{Opcode, SERVER_NAME_ES};
use num_traits::FromPrimitive;
use spinor::Spinor;
use xous::{msg_blocking_scalar_unpack, MemoryRange};

/*
!!!! EARLY SETTINGS ALLOCATED SLOTS !!!!

If you'll ever need a data slot in the early settings FLASH section,
please document it here for reference and use the Slot struct!

Thanks!

Keymap: offset 0, size 4
Early sleep: offset 4, size 4
*/

const KEYMAP: Slot = Slot { offset: 0, size: 4 };
const EARLY_SLEEP: Slot = Slot { offset: 4, size: 4 };

struct Slot {
    offset: u32,
    size: u32,
}

impl Slot {
    fn range(&self) -> Range<usize> {
        (self.offset as usize)..((self.offset + self.size) as usize)
    }
}

struct State {
    settings_page: Option<MemoryRange>,
    spinor: Spinor,
}

impl State {
    fn set(&self, data: &[u8], slot: &Slot) {
        let settings = match self.settings_page {
            Some(page) => page,
            None => return,
        };

        let settings: &[u8] = settings.as_slice();

        let new_data_u32 = u32::from_le_bytes(data.try_into().unwrap());

        if new_data_u32 == self.get(slot) {
            return;
        }

        self.spinor
            .patch(settings, xous::EARLY_SETTINGS, &data, slot.offset)
            .expect("couldn't patch slot data");
    }

    fn get(&self, slot: &Slot) -> u32 {
        let settings = match self.settings_page {
            Some(page) => page,
            None => return 0,
        };

        let settings: &[u8] = settings.as_slice();

        u32::from_le_bytes(settings[slot.range()].try_into().unwrap())
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    let state = State {
        settings_page: page_provider(),
        spinor: spinor::Spinor::new(&xns).unwrap(),
    };

    let sid = xns
        .register_name(SERVER_NAME_ES, None)
        .expect("can't register server");

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::GetKeymap) => {
                xous::return_scalar(msg.sender, state.get(&KEYMAP) as usize).unwrap();
            }
            Some(Opcode::SetKeymap) => msg_blocking_scalar_unpack!(msg, map, _, _, _, {
                let code = (map as u32).to_le_bytes();
                state.set(&code, &KEYMAP);
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::SetEarlySleep) => msg_blocking_scalar_unpack!(msg, value, _, _, _, {
                let code = (value as u32).to_le_bytes();
                state.set(&code, &EARLY_SLEEP);
                xous::return_scalar(msg.sender, 0).unwrap();
            }),
            Some(Opcode::EarlySleep) => {
                xous::return_scalar(msg.sender, state.get(&EARLY_SLEEP) as usize).unwrap();
            }
            _ => log::warn!("unrecognized opcode"),
        }
    }
}

fn page_provider() -> Option<xous::MemoryRange> {
    #[cfg(not(target_os = "xous"))]
    return None;

    #[cfg(target_os = "xous")]
    return Some(
        xous::syscall::map_memory(
            xous::MemoryAddress::new((xous::EARLY_SETTINGS + xous::FLASH_PHYS_BASE) as usize),
            None,
            4096,
            xous::MemoryFlags::R,
        )
        .unwrap(),
    );
}
