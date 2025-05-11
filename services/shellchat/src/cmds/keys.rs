use String;
use keystore_api::{AesRootkeyType, Block};

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
#[cfg(feature = "spinortest")]
pub struct Keys {
    testing_range: xous::MemoryRange,
    spinor: spinor::Spinor,
    rootkeys: root_keys::RootKeys,
}
#[derive(Debug)]
#[allow(dead_code)]
#[cfg(not(feature = "spinortest"))]
pub struct Keys {
    spinor: spinor::Spinor,
    rootkeys: root_keys::RootKeys,
}
#[cfg(feature = "spinortest")]
const TEST_SIZE: usize = 0x4000;
#[cfg(feature = "spinortest")]
const TEST_BASE: usize = 0x608_0000;
impl Keys {
    pub fn new(xns: &xous_names::XousNames) -> Keys {
        #[cfg(all(any(feature="precursor", feature="renode"), feature="spinortest"))]
        let testing_range = xous::syscall::map_memory(
            Some(core::num::NonZeroUsize::new(TEST_BASE + xous::FLASH_PHYS_BASE as usize).unwrap()), // occupy the 44.1khz short sample area for testing
            None,
            TEST_SIZE,
            xous::MemoryFlags::R,
        ).expect("couldn't map in testing range");
        #[cfg(all(not(any(feature = "precursor", feature = "renode")), feature = "spinortest"))]
        // just make a dummy mapping to keep things from crashing in hosted mode
        let testing_range = xous::syscall::map_memory(None, None, TEST_SIZE, xous::MemoryFlags::R)
            .expect("couldn't map in a fake testing range for hosted mode");

        let spinor = spinor::Spinor::new(&xns).unwrap();
        // NOTE NOTE NOTE -- this should be removed once we have the SoC updater code written, but for testing
        // we occupy this slot as it is a pre-requisite for the block to work
        spinor.register_soc_token().unwrap();

        #[cfg(feature = "spinortest")]
        let keys = Keys {
            testing_range,
            spinor,
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0))
                .expect("couldn't allocate rootkeys API"),
        };

        #[cfg(not(feature = "spinortest"))]
        let keys = Keys {
            spinor,
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0))
                .expect("couldn't allocate rootkeys API"),
        };
        keys
    }
}

impl<'a> ShellCmdApi<'a> for Keys {
    cmd_api!(keys);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "keys [usblock] [usbunlock] [pddbrecycle]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "ux" => {
                    self.rootkeys.test_ux(0);
                    //debug_here::debug_here!();
                    write!(ret, "show UX").unwrap();
                }
                "bbram" => {
                    self.rootkeys.bbram_provision();
                    write!(ret, "Provisioning BBRAM").unwrap();
                }
                "aes" => {
                    use root_keys::{BlockDecrypt, BlockEncrypt};
                    let mut pass = true;
                    let mut block = Block::clone_from_slice(&[0; 16]);
                    self.rootkeys.encrypt_block(&mut block);
                    log::info!("encrypted block: {:?}", block);
                    let mut sum = 0;
                    for &b in block.as_slice().iter() {
                        sum += b as u32; // we don't use .sum() because we need more bits than a u8
                    }
                    if sum == 0 {
                        pass = false;
                    }
                    // this will cause a second password box to pop up if you told it to use-once.
                    self.rootkeys.decrypt_block(&mut block);
                    log::info!("decrypted block: {:?}", block);
                    for &b in block.as_slice().iter() {
                        if b != 0 {
                            pass = false;
                        }
                    }
                    if pass {
                        write!(ret, "aes test passed").unwrap();
                    } else {
                        write!(ret, "aes test failed").unwrap();
                    }
                }
                "pddbrecycle" => {
                    // erase the page table, which should effectively trigger a reformat on the next boot
                    self.spinor
                        .bulk_erase(precursor_hal::board::PDDB_LOC, 1024 * 1024)
                        .expect("couldn't erase page table");
                    write!(ret, "PDDB page table erased").unwrap();
                }
                #[cfg(feature = "spinortest")]
                "spinortest" => {
                    let region = self.testing_range.as_slice::<u8>();
                    let region_base = TEST_BASE as u32; // base offsets are 0-relative to start of FLASH

                    log::debug!("top of region: {:x?}", &region[0..8]);
                    // a region to stash a copy of the previous data in the testing area, so we can diff to
                    // confirm things worked!
                    let mut reference_region = xous::syscall::map_memory(
                        None,
                        None,
                        TEST_SIZE,
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    )
                    .expect("couldn't allocate a reference region for testing flash data");
                    let reference = reference_region.as_slice_mut::<u8>();

                    for byte in reference.iter_mut() {
                        *byte = 0xFF;
                    }
                    log::debug!("erasing region");
                    self.spinor
                        .patch(&region, region_base, &reference, 0)
                        .expect("couldn't erase region for testing");

                    for (addr, &byte) in region.iter().enumerate() {
                        if byte != 0xFF {
                            log::error!("erase did not succeed at offset 0x{:x}", addr);
                        }
                    }
                    log::debug!("erase check finished");

                    // test a simple patch on erased data, at an unaligned address -- the smallest quantum is
                    // 2 bytes
                    log::debug!("simple patch test");
                    let patch: [u8; 2] = [0x33, 0xCC];
                    self.spinor.patch(&region, region_base, &patch, 4).expect("couldn't do smallest patch");
                    for (addr, &byte) in region.iter().enumerate() {
                        match addr {
                            4 => {
                                if byte != 0x33 {
                                    log::error!("smallpatch failed e.33 o.{:02x}", byte)
                                }
                            }
                            5 => {
                                if byte != 0xCC {
                                    log::error!("smallpatch failed e.cc o.{:02x}", byte)
                                }
                            }
                            _ => {
                                if byte != 0xFF {
                                    log::error!(
                                        "smallpatch failed, erase disturb at region offset 0x{:08x}",
                                        addr
                                    )
                                }
                            }
                        }
                    }
                    log::debug!("simple patch test finished");

                    // fill the region with random data
                    log::debug!("fill random data");
                    use rand_core::RngCore;
                    env.trng
                        .try_fill_bytes(reference)
                        .expect("couldn't fill reference region with random data");
                    self.spinor
                        .patch(&region, region_base, &reference, 0)
                        .expect("couldn't fill test region with random data");

                    let mut errs = 0;
                    for (addr, (&rom, &reference)) in region.iter().zip(reference.iter()).enumerate() {
                        if rom != reference {
                            if (errs % 256 < 2) || (errs % 256 >= 254) {
                                log::error!(
                                    "fill random failed 0x{:08x}: e.{:02x} o.{:02x}",
                                    addr,
                                    reference,
                                    rom
                                );
                            }
                            errs += 1;
                        }
                    }
                    log::debug!("fill random data finished, errs: {}", errs);

                    // test a patch on random data
                    log::debug!("programmed patch test");
                    let patch: [u8; 4] = [0xaa, 0xbb, 0xcc, 0xdd];
                    self.spinor.patch(&region, region_base, &patch, 12).expect("couldn't do smallest patch");
                    for (addr, (&byte, &reference)) in region.iter().zip(reference.iter()).enumerate() {
                        match addr {
                            12 => {
                                if byte != 0xaa {
                                    log::error!("patch failed e.aa o.{:02x}", byte)
                                }
                            }
                            13 => {
                                if byte != 0xbb {
                                    log::error!("patch failed e.bb o.{:02x}", byte)
                                }
                            }
                            14 => {
                                if byte != 0xcc {
                                    log::error!("patch failed e.cc o.{:02x}", byte)
                                }
                            }
                            15 => {
                                if byte != 0xdd {
                                    log::error!("patch failed e.dd o.{:02x}", byte)
                                }
                            }
                            _ => {
                                if byte != reference {
                                    if (errs % 256 < 4) || (errs % 256 >= 252) {
                                        log::error!(
                                            "patch failed, erase disturb at region offset 0x{:08x}: e.{:02x} o.{:02x}",
                                            addr,
                                            reference,
                                            byte
                                        )
                                    }
                                    errs += 1;
                                }
                            }
                        }
                    }
                    log::debug!("patch test finished, errs {}", errs);
                    // refresh the reference, otherwise this patch will throw errors
                    for (&src, dst) in region.iter().zip(reference.iter_mut()) {
                        *dst = src;
                    }

                    log::debug!("patch across a page boundary");
                    let bigger_patch: [u8; 16] = [
                        0xf0, 0x0d, 0xbe, 0xef, 0xaa, 0x55, 0x99, 0x66, 0xff, 0xff, 0x00, 0x00, 0xff, 0xff,
                        0xff, 0xff,
                    ];
                    log::debug!("before patch: {:x?}", &region[0x0ff4..0x100c]);
                    self.spinor
                        .patch(&region, region_base, &bigger_patch, 0x0FF8)
                        .expect("can't patch across page boundary");
                    let mut i = 0;
                    errs = 0;
                    for (addr, (&rom, &reference)) in region.iter().zip(reference.iter()).enumerate() {
                        match addr {
                            0xff8..=0x1007 => {
                                if rom != bigger_patch[i] {
                                    if (errs % 256 < 2) || (errs % 256 >= 254) {
                                        log::error!(
                                            "bigger patch failed 0x{:08x}: e.{:02x} o.{:02x}",
                                            addr,
                                            bigger_patch[i],
                                            rom
                                        );
                                    }
                                    errs += 1;
                                }
                                i += 1;
                            }
                            _ => {
                                if rom != reference {
                                    if (errs % 256 < 2) || (errs % 256 >= 254) {
                                        log::error!(
                                            "data disturbed at 0x{:08x}: e.{:02x} o.{:02x}",
                                            addr,
                                            reference,
                                            rom
                                        );
                                    }
                                    errs += 1;
                                }
                            }
                        }
                    }
                    log::debug!("after patch: {:x?}", &region[0x0ff4..0x100c]);
                    log::debug!("patch across a page boundary finished, errs: {}", errs);

                    write!(ret, "Finished SPINOR primitives test (see serial log for pass/fail details).")
                        .unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
