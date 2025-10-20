use core::convert::TryInto;

use bao1x_api::{
    CP_ID, IFR_CP_ID_BASE, IFR_HASH, SERIAL_NUMBER, SLOT_ELEMENT_LEN_BYTES, SlotIndex, SlotType, UUID,
};
use bao1x_hal::{
    acram::{AccessSettings, SlotManager},
    rram::Reram,
};

/// This function is designed to be called "frequently" to audit the state
/// of the security slots. Its role is to:
///    - Initialize any keys that have not yet been initialized
///    - Update the one-way counters to lock out re-initializations
///    - Verify that the ACLs have been set according to the spec
///    - Repair any ACLs that are not set correctly. "Repair" is expected on first boot at the factory, since
///      the chip comes blank. But repair is also useful in case an adversary manages to flip any ACL states
///      on us.
pub fn check_slots(board_type: &bao1x_api::BoardTypeCoding) {
    let owc = bao1x_hal::acram::OneWayCounter::new();
    let mut slot_mgr = bao1x_hal::acram::SlotManager::new();
    let mut maybe_trng: Option<super::trng::ManagedTrng> = None;
    let mut rram = Reram::new();

    // check the lifecycle status
    if owc.get(bao1x_api::CP_BOOT_SETUP_DONE).unwrap() == 0 {
        crate::println!("CP setup not yet done. Initializing public identifiers...");
        // CP boot setup is not done. Initialize the basic identifiers.
        // Each ID is checked first to see if it is non-zero. If it is non-zero, don't replace it.
        // The reason this is done is because the ID capture/generation may happen at a time when
        // power is unstable, so we may be recovering from a previous early power-down.

        // Note the test condition b == 0 || b == 0xFF allows for a mix of 0x0 or 0xFF *only* as a
        // trigger condition to update the values. This is intentional, as after CP the array may have
        // some locations that are all 0 or all 1 (there's a checkerboard pattern test as part of the
        // CP). However, all of the values under test should be *random* and thus a value that consists
        // solely of those two numbers is highly unlikely to occur.

        // Grab the CP ID first. This is the hardest one to capture as it is only available for a fixed time
        // during CP and we can't strictly control how long we are powered on for.
        if slot_mgr.read(&CP_ID).unwrap().iter().all(|&b| b == 0 || b == 0xFF) {
            let cp_id =
                unsafe { core::slice::from_raw_parts(IFR_CP_ID_BASE as *const u8, SLOT_ELEMENT_LEN_BYTES) };
            slot_mgr.write(&mut rram, &CP_ID, cp_id.try_into().unwrap()).unwrap();
        }

        // this one may allocate a TRNG, which takes >50 ms to warm up.
        if slot_mgr.read(&SERIAL_NUMBER).unwrap().iter().all(|&b| b == 0 || b == 0xFF) {
            let trng = maybe_trng.get_or_insert_with(|| super::trng::ManagedTrng::new(&board_type));
            let k = trng.generate_key();
            slot_mgr.write(&mut rram, &SERIAL_NUMBER, &k).unwrap();
        }
        if slot_mgr.read(&UUID).unwrap().iter().all(|&b| b == 0 || b == 0xFF) {
            let trng = maybe_trng.get_or_insert_with(|| super::trng::ManagedTrng::new(&board_type));
            let k = trng.generate_key();
            slot_mgr.write(&mut rram, &UUID, &k).unwrap();
        }
        if slot_mgr.read(&IFR_HASH).unwrap().iter().all(|&b| b == 0 || b == 0xFF) {
            use digest::Digest;
            use sha2_bao1x::Sha256;
            let mut hasher = Sha256::new();
            let ifr_slice =
                unsafe { core::slice::from_raw_parts(bao1x_api::IFR_BASE as *const u8, bao1x_api::IFR_LEN) };
            hasher.update(&ifr_slice);
            let digest = hasher.finalize();
            slot_mgr.write(&mut rram, &IFR_HASH, digest.as_slice().try_into().unwrap()).unwrap();
        }
        // once all values are written, advance the CP_BOOT_SETUP_DONE state
        // safety: the offset is correct because we're pulling it from our pre-defined constants and
        // those are manually checked.
        unsafe { owc.inc(bao1x_api::CP_BOOT_SETUP_DONE).unwrap() };
        crate::println!("Public ID init done.");
    }

    if *board_type == bao1x_api::BoardTypeCoding::Baosec
        && owc.get(bao1x_api::IN_SYSTEM_BOOT_SETUP_DONE).unwrap() == 0
    {
        crate::println!("System setup not yet done. Initializing secret identifiers...");
        let trng = maybe_trng.get_or_insert_with(|| super::trng::ManagedTrng::new(&board_type));
        // generate all the keys
        for key_range in bao1x_api::baosec::KEY_SLOTS.iter() {
            let mut storage = alloc::vec::Vec::with_capacity(key_range.len() * SLOT_ELEMENT_LEN_BYTES);
            storage.resize(key_range.len() * SLOT_ELEMENT_LEN_BYTES, 0);
            for chunk in storage.chunks_mut(SLOT_ELEMENT_LEN_BYTES) {
                chunk.copy_from_slice(&trng.generate_key());
            }
            slot_mgr.write(&mut rram, key_range, &storage).unwrap();
        }
        // once all values are written, advance the IN_SYSTEM_BOOT_SETUP_DONE state
        // safety: the offset is correct because we're pulling it from our pre-defined constants and
        // those are manually checked.
        unsafe { owc.inc(bao1x_api::IN_SYSTEM_BOOT_SETUP_DONE).unwrap() };
        crate::println!("Secret ID init done.");
    }
    print_slots(&slot_mgr, &bao1x_hal::board::DATA_SLOTS);
    check_and_fix_acls(&mut rram, &mut slot_mgr, &bao1x_hal::board::DATA_SLOTS);
    if *board_type == bao1x_api::BoardTypeCoding::Baosec {
        #[cfg(feature = "unsafe-debug")]
        print_slots(&slot_mgr, &bao1x_hal::board::KEY_SLOTS);
        check_and_fix_acls(&mut rram, &mut slot_mgr, &bao1x_hal::board::KEY_SLOTS);
    }
}

fn check_and_fix_acls(rram: &mut Reram, slot_mgr: &mut SlotManager, slot_list: &[SlotIndex]) {
    // now check & set any ACL bits that aren't set yet
    for slot_element in slot_list.iter() {
        let mut is_consistent = true;
        let mut acl = match slot_mgr.get_acl(&slot_element) {
            Ok(settings) => settings,
            Err(bao1x_api::AccessError::KeyAclInconsistency(prototype)) => {
                is_consistent = false;
                AccessSettings::Key(prototype)
            }
            Err(bao1x_api::AccessError::DataAclInconsistency(prototype)) => {
                is_consistent = false;
                AccessSettings::Data(prototype)
            }
            _ => panic!("Unhandled error in get_acl()"),
        };
        let (pa, rw) = slot_element.get_access_spec();
        let is_correct = match acl {
            AccessSettings::Data(sa) => sa.get_partition_access() == pa && sa.get_rw_permissions() == rw,
            AccessSettings::Key(sa) => sa.get_partition_access() == pa && sa.get_rw_permissions() == rw,
        };
        if !is_correct || !is_consistent {
            crate::println!(
                "Fixing ACL for {:?} (is_correct: {:?}, is_consistent: {:?})",
                slot_element,
                is_correct,
                is_consistent
            );
            match &mut acl {
                AccessSettings::Data(sa) => {
                    sa.set_partition_access(&pa);
                    sa.set_rw_permissions(rw);
                }
                AccessSettings::Key(sa) => {
                    sa.set_partition_access(&pa);
                    sa.set_rw_permissions(rw);
                }
            }
            slot_mgr.set_acl(rram, slot_element, &acl).unwrap();
        }
    }
}

fn print_slots(slot_mgr: &SlotManager, slot_list: &[SlotIndex]) {
    for slot in slot_list.iter() {
        let access = slot.get_access_spec();
        crate::println!("== Slot starting at {} ==", slot.get_base());
        crate::println!("  Spec permissions: {:?}", access);
        let slot_type = slot.get_type();
        for (data_index, acl_index) in
            slot.try_into_data_iter().unwrap().zip(slot.try_into_acl_iter().unwrap())
        {
            match slot_type {
                // safety: we have checked the slot type before entering these low level functions
                SlotType::Data => unsafe {
                    let bytes = slot_mgr.read_data_slot(data_index);
                    let acl = slot_mgr.get_data_acl(acl_index);
                    crate::println!("    Data {} ({:?}): {:x?}", data_index, acl, bytes);
                },
                SlotType::Key => unsafe {
                    let bytes = slot_mgr.read_key_slot(data_index);
                    let acl = slot_mgr.get_key_acl(acl_index);
                    crate::println!("    Key {} ({:?}): {:x?}", data_index, acl, bytes);
                },
            }
        }
    }
}
