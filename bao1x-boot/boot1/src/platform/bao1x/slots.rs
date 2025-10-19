/// This function is designed to be called "frequently" to audit the state
/// of the security slots. Its role is to:
///    - Initialize any keys that have not yet been initialized
///    - Update the one-way counters to lock out re-initializations
///    - Verify that the ACLs have been set according to the spec
///    - Repair any ACLs that are not set correctly. "Repair" is expected on first boot at the factory, since
///      the chip comes blank. But repair is also useful in case an adversary manages to flip any ACL states
///      on us.
pub fn check_slots(board_type: &bao1x_api::BoardTypeCoding) {}
