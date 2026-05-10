// SPDX-License-Identifier: GPL-3.0-only
//
//! OpenPGP CCID USB class setup for `ccid-openpgp` (master key, PIN load, optional CDC provisioning).

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;

use bao1x_hal::rram::Reram;
use bao1x_hal::usb::driver::CorigineWrapper;
use galdr_core::HalError;
use usb_device::class_prelude::UsbBusAllocator;
use usb_personality::ccid::CcidClass;
use usb_personality::openpgp::build_aid;
use usb_personality::openpgp::OpenPgpCcidDispatcher;
use utralib::AtomicCsr;
use xous_names::XousNames;

use crate::provisioning;

pub(crate) fn build_ccid_and_rram<'a>(
    xns: &XousNames,
    usb_csr: &AtomicCsr<u32>,
    irq_csr: &AtomicCsr<u32>,
    cw: &CorigineWrapper,
    usb_alloc: &'a UsbBusAllocator<CorigineWrapper>,
    serial_number: &str,
) -> Result<
    (
        CcidClass<'a, CorigineWrapper, OpenPgpCcidDispatcher<baochip_openpgp::BaochipVaultBackend>>,
        Rc<RefCell<Reram>>,
    ),
    HalError,
> {
    let mut reram = Reram::new();
    baochip_openpgp::map_openpgp_rram_windows(Pin::as_mut(&mut reram)).map_err(|_| HalError::Bus)?;
    let shared = Rc::new(RefCell::new(reram));

    let aid = build_aid(0x0000, [1, 2, 3, 4]);

    #[cfg(feature = "ccid-openpgp-dev")]
    {
        let master_key =
            baochip_openpgp::master_key_dev_from_env().map_err(|_| HalError::Denied)?;
        let (u, a) = baochip_openpgp::ccid_pins_dev_from_env().map_err(|_| HalError::Denied)?;
        let backend = baochip_openpgp::open_or_provision_backend(
            shared.clone(),
            xns,
            master_key,
            aid,
            u.as_slice(),
            a.as_slice(),
        )?;
        let ccid = CcidClass::new(usb_alloc, OpenPgpCcidDispatcher::new(backend));
        return Ok((ccid, shared));
    }

    #[cfg(not(feature = "ccid-openpgp-dev"))]
    {
        let mut trng_boot = trng::Trng::new(xns).map_err(|_| HalError::Bus)?;
        let master_key = baochip_openpgp::load_or_derive_ccid_master_key(
            &shared,
            &mut trng_boot,
            serial_number.as_bytes(),
        )?;

        match baochip_openpgp::open_or_provision_backend(
            shared.clone(),
            xns,
            master_key,
            aid,
            &[],
            &[],
        ) {
            Ok(backend) => {
                let ccid = CcidClass::new(usb_alloc, OpenPgpCcidDispatcher::new(backend));
                Ok((ccid, shared))
            }
            Err(HalError::NeedsProvisioning) => {
                if !baochip_openpgp::ccid_pin_hashes_unprovisioned(&shared) {
                    return Err(HalError::Denied);
                }
                provisioning::run_first_boot_pin_provisioning(
                    usb_csr,
                    irq_csr,
                    cw,
                    serial_number,
                    &shared,
                )?;
                let user_pin =
                    baochip_openpgp::load_or_provision_ccid_user_pin_bytes(&shared, &mut trng_boot)?;
                let admin_pin =
                    baochip_openpgp::load_or_provision_ccid_admin_pin_bytes(&shared, &mut trng_boot)?;
                let backend = baochip_openpgp::open_or_provision_backend(
                    shared.clone(),
                    xns,
                    master_key,
                    aid,
                    user_pin.as_slice(),
                    admin_pin.as_slice(),
                )?;
                let ccid = CcidClass::new(usb_alloc, OpenPgpCcidDispatcher::new(backend));
                Ok((ccid, shared))
            }
            Err(e) => Err(e),
        }
    }
}
