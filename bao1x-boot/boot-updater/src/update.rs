use bao1x_api::pubkeys::*;
use bao1x_api::*;
use bao1x_hal::hardening::*;

#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

#[unsafe(link_section = ".payload")]
#[cfg(feature = "boot0")]
static BOOT0: &Aligned<[u8]> = &Aligned(*include_bytes!(env!("BOOT0_BIN")));

#[unsafe(link_section = ".payload")]
static BOOT1: &Aligned<[u8]> = &Aligned(*include_bytes!(env!("BOOT1_BIN")));

fn detect_stepping() -> &'static str {
    let mut rram = utralib::CSR::new(utralib::utra::rrc::HW_RRC_BASE as *mut u32);
    // this sets bit 12
    rram.wfo(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);
    // attempt to clear bit 12
    rram.wfo(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE & !(1 << 12));
    let check_val = rram.rf(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR);
    let ret = if check_val != bao1x_hal::rram::SECURITY_MODE { "A0" } else { "A1" };
    // reset security mode
    bollard!(die, 4);
    rram.wfo(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);
    bollard!(die, 4);
    rram.wfo(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);
    bollard!(die, 4);
    rram.wfo(utralib::utra::rrc::SFR_RRCCR_SFR_RRCCR, bao1x_hal::rram::SECURITY_MODE);
    bollard!(die, 4);
    ret
}

pub fn run(mut csprng: &mut Csprng) -> bool {
    let mut passing = true;
    bollard!(die, 4);
    // double-check boot0 signatures on uploaded artifacts
    // hardened because this could be a vector to upload arbitrary images with a glitch attack
    #[cfg(feature = "boot0")]
    {
        let boot0_check = SecurityConfiguration {
            image_ptr: BOOT0.0.as_ptr() as *const u32,
            pubkey_ptr: BOOT0_SELF_CHECK.pubkey_ptr,
            revocation_owc: BOOT0_SELF_CHECK.revocation_owc,
            function_codes: BOOT0_SELF_CHECK.function_codes,
        };
        csprng.random_delay();
        let boot0_check1 =
            bao1x_hal::sigcheck::validate_image(boot0_check, None, Some(&mut csprng), HardenedBool::TRUE)
                .unwrap_or_else(|_| die());
        if boot0_check1.0 != !boot0_check1.1 {
            crate::println!("boot0 failed check");
            die();
        }
        csprng.random_delay();
        let boot0_check2 =
            bao1x_hal::sigcheck::validate_image(boot0_check, None, Some(&mut csprng), HardenedBool::TRUE)
                .unwrap_or_else(|_| die());
        bollard!(die, 4);
        if boot0_check2.0 != !boot0_check2.1 {
            bollard!(die, 4);
            crate::println!("boot0 failed check");
            die();
        }
    }
    csprng.random_delay();

    // double-check boot1 signatures
    let boot1_check = SecurityConfiguration {
        image_ptr: BOOT1.0.as_ptr() as *const u32,
        pubkey_ptr: BOOT0_TO_BOOT1.pubkey_ptr,
        revocation_owc: BOOT0_TO_BOOT1.revocation_owc,
        function_codes: BOOT0_TO_BOOT1.function_codes,
    };
    csprng.random_delay();
    let boot1_check1 =
        bao1x_hal::sigcheck::validate_image(boot1_check, None, Some(&mut csprng), HardenedBool::TRUE)
            .unwrap_or_else(|_| die());
    if boot1_check1.0 != !boot1_check1.1 {
        crate::println!("boot1 failed check");
        die();
    }
    csprng.random_delay();
    let boot1_check2 =
        bao1x_hal::sigcheck::validate_image(boot1_check, None, Some(&mut csprng), HardenedBool::TRUE)
            .unwrap_or_else(|_| die());
    bollard!(die, 4);
    if boot1_check2.0 != !boot1_check2.1 {
        bollard!(die, 4);
        crate::println!("boot1 failed check");
        die();
    }

    crate::println!("All signature checks passed!");

    bollard!(die, 4);
    let mut rram = crate::rram::Reram::new();

    crate::println!("Writing boot1...");
    // strip off the absolute address prefix so we have the relative offset
    match unsafe { rram.crazy_unsafe_write_slice(BOOT1_START & 0x0FFF_FFFF, &BOOT1.0) } {
        Err(e) => {
            passing = false;
            crate::println!("{:?}: hardware error in RRAM write, device likely bricked", e);
        }
        _ => (),
    }

    crate::println!("  Checking boot1");
    // we trigger boot1 ARB increment here because the only option is pass or brick
    let boot1_verify =
        bao1x_hal::sigcheck::validate_image(BOOT0_TO_BOOT1, None, Some(&mut csprng), HardenedBool::FALSE);
    match &boot1_verify {
        Ok(_) => {
            crate::println!("  boot1 update passed!")
        }
        Err(e) => {
            passing = false;
            crate::println!(
                "  {:?}: boot1 update failed. This error is unrecoverable, board is now a brick.",
                e
            );
        }
    }

    // implement the boot0 update iff version is A0
    #[cfg(feature = "boot0")]
    {
        // check if we're an A0 rev device
        let stepping = detect_stepping();

        if stepping == "A0" {
            // Note: the A0 security bypass is in the RRAM implementation

            crate::println!("Writing boot0...");
            // strip off the absolute address prefix so we have the relative offset
            match unsafe { rram.crazy_unsafe_write_slice(BOOT0_START & 0x0FFF_FFFF, &BOOT0.0) } {
                Err(e) => {
                    passing = false;
                    crate::println!("  {:?}: hardware error in RRAM write, device likely bricked", e);
                }
                _ => {}
            }

            crate::println!("  Checking boot0");
            // we do not increment the ARB here because there is a saving throw still
            let boot0_verify = bao1x_hal::sigcheck::validate_image(
                BOOT0_SELF_CHECK,
                None,
                Some(&mut csprng),
                HardenedBool::TRUE,
            );
            match &boot0_verify {
                Ok(_) => {
                    crate::println!("  boot0 update passed!")
                }
                Err(e) => match &boot1_verify {
                    Ok((_key, _key_inv, tag, target, _pq_tag)) => {
                        crate::println!(
                            "  {:?}, boot0 update failed. Dropping to boot1, there is a chance to upload a new image there and retry. Roll a d20 while you're at it.",
                            e
                        );
                        jump_to(*target as usize, u32::from_le_bytes(*tag) as usize);
                    }
                    Err(e) => {
                        passing = false;
                        crate::println!(
                            "{:?}, boot0 & boot1 failed update, unrecoverable error. Device is now a brick.",
                            e
                        )
                    }
                },
            }
        } else {
            crate::println!("Can't patch boot0 on A1 silicon, skipping boot0 update!");
        }
    }

    crate::println!("Updater finished");
    passing
}

pub fn jump_to(target: usize, mask: usize) -> ! {
    // loader expects a0 to have the address of the kernel image pre-loaded
    let kernel_loc = bao1x_api::offsets::KERNEL_START;
    unsafe {
        core::arch::asm!(
            "mv t0, {target}",
            "mv t1, {mask}",
            "mv a0, {kernel_loc}",
            "mv a1, x0",
            "xor t0, t1, t0",
            "jr t0",
            target = in(reg) target,
            mask = in(reg) mask,
            kernel_loc = in(reg) kernel_loc,
            options(noreturn)
        );
    }
}

/*
// this was used just for security testing. Maybe useful in the future for audits.

use core::arch::naked_asm;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn enter_supervisor_asid3() {
    naked_asm!(
        // --- set mstatus.MPP = 01 (Supervisor) ---
        "csrr t0, mstatus",
        "li   t1, 0x1800", // MPP mask, bits 12:11
        "not  t1, t1",
        "and  t0, t0, t1",
        "li   t1, 0x800", // MPP = 01
        "or   t0, t0, t1",
        "csrw mstatus, t0",
        // --- point mepc at our return address so mret acts like `ret` ---
        "csrw mepc, ra",
        // --- satp: RV32 layout, MODE=Bare(0), ASID=3, PPN=0 ---
        "li   t0, 0x00C00000",
        "csrw satp, t0",
        // --- go: privilege becomes S, PC becomes old ra ---
        "mret",
    );
}
*/
