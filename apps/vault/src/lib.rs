// Copyright 2019-2022 Google LLC
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

use crate::ctap::hid::HidPacket;
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
#[cfg(feature = "std")]
pub mod test_helpers;

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
