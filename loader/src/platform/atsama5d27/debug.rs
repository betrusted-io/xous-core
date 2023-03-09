// SPDX-FileCopyrightText: 2022 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use atsama5d27::uart::{Uart as UartHw, Uart1};
use armv7::structures::paging::{
    PageTable as L2PageTable, TranslationTable,
    TranslationTableType, PageTableType,
    TranslationTableMemory, PageTableMemory,
};
use armv7::PhysicalAddress;

type UartType = UartHw<Uart1>;

pub struct Uart {
    inner: UartType,
}

impl Uart {
    pub fn new() -> Self {
        Self {
            inner: UartType::new(),
        }
    }
}

use core::fmt::{Error, Write};

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        self.inner.write_str(s);
        Ok(())
    }
}

#[macro_use]
#[cfg(all(not(test), feature = "debug-print"))]
pub mod debug_print_hardware {
    #[macro_export]
    macro_rules! print
    {
        ($($args:tt)+) => ({
                use core::fmt::Write;
                let _ = write!($crate::debug::Uart::new(), $($args)+);
        });
    }
}

#[macro_use]
#[cfg(all(not(test), not(feature = "debug-print")))]
mod debug_print_hardware {
    #[macro_export]
    #[allow(unused_variables)]
    macro_rules! print {
        ($($args:tt)+) => {{}};
    }
}

#[macro_use]
#[cfg(test)]
mod debug_print_hardware {
    #[macro_export]
    #[allow(unused_variables)]
    macro_rules! print {
        ($($args:tt)+) => ({
            std::print!($($args)+)
        });
    }
}

#[macro_export]
macro_rules! println
{
    () => ({
        $crate::print!("\r\n")
    });
    ($fmt:expr) => ({
        $crate::print!(concat!($fmt, "\r\n"))
    });
    ($fmt:expr, $($args:tt)+) => ({
        $crate::print!(concat!($fmt, "\r\n"), $($args)+)
    });
}

fn print_l2_pagetable(vpn1: usize, phys_addr: PhysicalAddress) {
    let ptr: *mut PageTableMemory = phys_addr.as_mut_ptr();
    let mut l2_pt = unsafe { L2PageTable::new_from_ptr(ptr) };
    let l2_pt = unsafe { l2_pt.table_mut() };

    let mut no_valid_items = true;
    for (i, pt_desc) in l2_pt.iter().enumerate() {
        let virt_addr = (vpn1 << 20) | (i << 12);

        if let PageTableType::Invalid = pt_desc.get_type() {
            continue;
        }

        no_valid_items = false;

        let phys_addr = pt_desc.get_addr().expect("addr");

        match pt_desc.get_type() {
            PageTableType::LargePage => println!(
                "        - {:02x} (64K) Large Page {:08x} -> {:08x}",
                i, virt_addr, phys_addr
            ),
            PageTableType::SmallPage => println!(
                "        - {:02x} (4K)  Small Page {:08x} -> {:08x}",
                i, virt_addr, phys_addr
            ),
            _ => (),
        }
    }

    if no_valid_items {
        println!("        - <no valid items>");
    }
}

pub fn print_pagetable(root: usize) {
    println!("Memory Maps (Root: {:08x}):", root,);

    let tt_ptr = root as *mut TranslationTableMemory;
    let tt = TranslationTable::new(tt_ptr);

    for (i, tt_desc) in tt.table().iter().enumerate() {
        if let TranslationTableType::Invalid = tt_desc.get_type() {
            continue;
        }

        let phys_addr = tt_desc.get_addr().expect("addr");

        match tt_desc.get_type() {
            TranslationTableType::Page => {
                let virt_addr = i << 20;
                println!(
                    "    - {:03x} (1MB) {:08x} L2 page table @ {:08x}",
                    i, virt_addr, phys_addr
                );
                print_l2_pagetable(i, phys_addr);
            }
            TranslationTableType::Section => {
                let virt_addr = i * 1024; // 1 MB
                println!(
                    "    - {:03x} (1MB)  section {:08x} -> {:08x}",
                    i, virt_addr, phys_addr
                );
            }
            TranslationTableType::Supersection => {
                let virt_addr = i * (1024 * 16); // 16 MB
                println!(
                    "    - {:03x} (16MB) supersection {:08x} -> {:08x}",
                    i, virt_addr, phys_addr
                );
            }

            _ => (),
        }
    }
}
