use keystore_api::*;

mod platform;

fn main() -> ! {
    #[cfg(feature = "debug-hal")]
    bao1x_hal::claim_duart();
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // TODO: limit connections to this server?? once we know all that will connect
    let keys_sid = xns.register_name(SERVER_NAME_KEYS, None).expect("can't register server");

    platform::keystore(keys_sid);
}

/// This piece of code can be copy/pasted into any other process to test the integrity of the hardware
/// protection mechanism. It does two things:
///
/// 1. Maps the key store into its memory space
/// 2. Attempts to override the coreuser mappings to fake permissions to access the keys
///
/// For the test to even run, the keystore's mapping on the COREUSER peripheral *and* the `SlotManager`
/// need to be disabled. This is because the virtual memory system causes the keystore to "own" these
/// mappings which gives it exclusive access to these register banks.
///
/// Once those protections are bypassed, then, the coreuser mapping is a second line of protection.
/// This mapping *should* be set and locked down by the boot1 stage bootloader.
///
/// Thus any viable attack against this has to start with a chain before the one-way door is sealed
/// on this configuration. If the one-way door has been successfully defeated, you will be able to
/// read the keys out with this code.
///
/// Alternatively, one can find a kernel exploit that gains arbitrary exec in supervisor mode, which
/// will allow you to set the ASID arbitrarily. This would allow an attacker to spoof the PID of
/// the keystore process and thus access the keys.
///
/// Of course, this code running inside this process will work to read out the keys, since this
/// is the designated, trusted process for accessing the key store.
#[allow(dead_code)]
#[cfg(not(feature = "hosted-baosec"))]
fn attack_keystore() {
    use utralib::*;
    let cu_range = xous::map_memory(
        xous::MemoryAddress::new(utra::coreuser::HW_COREUSER_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .unwrap();
    let mut cu = CSR::new(cu_range.as_ptr() as *mut u32);
    log::info!("coreuser status: {:x}", cu.r(utra::coreuser::STATUS));

    cu.rmwf(utra::coreuser::MAP_HI_LUT7, 0);
    cu.rmwf(utra::coreuser::USERVALUE_USER7, bao1x_hal::coreuser::TRUSTED_USER.as_dense());
    log::info!("--------- COREUSER SET: {:x} -------------", cu.r(utra::coreuser::STATUS));

    let slot_mgr = bao1x_hal::acram::SlotManager::new();
    // let slot = bao1x_api::baosec::ROOT_SEED;
    let slot = bao1x_api::offsets::SlotIndex::Data(
        256,
        bao1x_api::PartitionAccess::Fw0,
        bao1x_api::RwPerms::ReadOnly,
    );
    let data_index = slot.try_into_data_offset().unwrap();
    let acl_index = slot.try_into_acl_offset().unwrap();
    let user_states = [
        bao1x_hal::coreuser::CoreuserId::Boot0,
        bao1x_hal::coreuser::CoreuserId::Boot1,
        bao1x_hal::coreuser::CoreuserId::Fw0,
        bao1x_hal::coreuser::CoreuserId::Fw1,
    ];
    for i in 0..2 {
        cu.rmwf(utra::coreuser::CONTROL_INVERT_PRIV, i % 2);
        for state in user_states {
            cu.rmwf(utra::coreuser::MAP_LO_LUT2, xous::process::id() as u32);
            cu.rmwf(utra::coreuser::USERVALUE_USER2, state.as_dense());
            log::info!(" COREUSER STATUS {:x}", cu.r(utra::coreuser::STATUS));
            unsafe {
                let bytes = slot_mgr.read_data_slot(data_index);
                let acl = slot_mgr.get_data_acl(acl_index);
                log::info!(
                    "    Key {} ({:x?}): {:x?}",
                    data_index / bao1x_api::offsets::SLOT_ELEMENT_LEN_BYTES,
                    acl,
                    bytes
                );
            }
        }
    }
}
