#![cfg_attr(rustfmt, rustfmt_skip)]
// Copyright2019-2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// #![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
extern crate arrayref;

use crate::ctap::storage::pin_hash;
use crate::ctap::{hid::HidPacket, storage::count_credentials};
use crate::ctap::main_hid::MainHid;
#[cfg(feature = "vendor_hid")]
use crate::ctap::vendor_hid::VendorHid;
use crate::ctap::CtapState;
pub use crate::ctap::Transport;
use crate::env::Env;
use std::time::{Instant, Duration};
#[cfg(feature="xous")]
use ctap::main_hid::HidIterType;

// Those macros should eventually be split into trace, debug, info, warn, and error macros when
// adding either the defmt or log feature and crate dependency.
#[cfg(feature = "debug_ctap")]
macro_rules! debug_ctap {
    ($env: expr, $($rest:tt)*) => {{
        use core::fmt::Write;
        writeln!($env.write(), $($rest)*).unwrap();
    }};
}

pub mod api;
// TODO(kaczmarczyck): Refactor this so that ctap module isn't public.
pub mod ctap;
pub mod env;

pub const KEEPALIVE_DELAY_MS: u64 = 100;
pub const KEEPALIVE_DELAY: Duration = Duration::from_millis(KEEPALIVE_DELAY_MS);

/// CTAP implementation parameterized by its environment.
pub struct Ctap<E: Env> {
    env: E,
    state: CtapState,
    hid: MainHid,
    #[cfg(feature = "vendor_hid")]
    vendor_hid: VendorHid,
}

impl<E: Env> Ctap<E> {
    /// Instantiates a CTAP implementation given its environment.
    // This should only take the environment, but it temporarily takes the boot time until the
    // clock is part of the environment.
    pub fn new(mut env: E, now: Instant) -> Self {
        let state = CtapState::new(&mut env, now);
        let hid = MainHid::new();
        #[cfg(feature = "vendor_hid")]
        let vendor_hid = VendorHid::new();
        Ctap {
            env,
            state,
            hid,
            #[cfg(feature = "vendor_hid")]
            vendor_hid,
        }
    }

    pub fn is_unused(&mut self) -> bool {
        let master_keys_exist = self.env.store().find_handle(crate::api::key_store::STORAGE_KEY).unwrap().is_some();
        let residential_credentials = count_credentials(&mut self.env).unwrap();
        let sig_counter_written = self.env.store().find_handle(crate::ctap::storage::key::GLOBAL_SIGNATURE_COUNTER).unwrap().is_some();
        let pin_set = pin_hash(&mut self.env).unwrap().is_some();

        log::debug!("master keys: {:?}", master_keys_exist);
        log::debug!("residentials: {}", residential_credentials);
        log::debug!("sig_counter_written: {:?}", sig_counter_written);
        log::debug!("pin_set: {:?}", pin_set);

        !master_keys_exist
            && residential_credentials == 0
            && !sig_counter_written
            && !pin_set
    }

    pub fn state(&mut self) -> &mut CtapState {
        &mut self.state
    }

    pub fn hid(&mut self) -> &mut MainHid {
        &mut self.hid
    }

    pub fn env(&mut self) -> &mut E {
        &mut self.env
    }

    pub fn process_hid_packet(
        &mut self,
        packet: &HidPacket,
        transport: Transport,
        now: Instant,
    ) -> HidIterType {
        match transport {
            Transport::MainHid => {
                self.hid
                    .process_hid_packet(&mut self.env, packet, now, &mut self.state)
            }
            #[cfg(feature = "vendor_hid")]
            Transport::VendorHid => {
                self.vendor_hid
                    .process_hid_packet(&mut self.env, packet, now, &mut self.state)
            }
        }
    }

    pub fn update_timeouts(&mut self, now: Instant) {
        self.state.update_timeouts(now);
        self.hid.update_wink_timeout(now);
    }
}

#[cfg(feature="xous")]
pub mod vault_api;
#[cfg(feature="xous")]
pub use vault_api::*;
