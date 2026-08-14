use std::io::{Read, Write};

use aes_gcm_siv::{Aes256GcmSiv, KeyInit, Nonce};
use bao1x_api::{IoSetup, IoxValue};
use dc34_api::*;
use num_traits::*;
use pddb::Pddb;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::VaultMode;

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum AttachState {
    /// Straight off the assembly line and first firmware load
    FactoryNew,
    /// Provisioned but still in the factory
    TestedStandAlone,
    /// State when plugged into a badge for the first time, and type is initialized
    FirstMate,
    /// State when plugged into a correct badge
    Matched,
    /// State after initialization, but removed from badge for e.g. "token mode"
    Unattached,
    /// State when initialized and plugged into an incorrect badge
    Mismatched,
}
impl AttachState {
    pub fn attached(&self) -> bool {
        *self == AttachState::FactoryNew
            || *self == AttachState::Matched
            || *self == AttachState::Mismatched
            || *self == AttachState::FirstMate
    }
}
/// Structure for tracking global shared state. Everything in here
/// needs to be suitable for sticking in an Arc/Mutex
pub(crate) struct GlobalConfig {
    is_developer: bool,
    attach_state: AttachState,
    was_cold_boot: bool,
    k0: [u8; 32],
    skip_tour: bool,
    skip_token_tour: bool,
    badge_type: BadgeType,
    led_server: xous::CID,
    power_server: xous::CID,
    display_fade_cache: bool,
    /// This is a cached copy of the gene - it sits in between the permanent copy
    /// in the PDDB, and the expressed copy inside the LightGene on the LED server.
    /// any changes to this are ephemeral and are also invisible until pushed to
    /// an endpoint. However, by keeping the gene in a volatile cached copy, we speed
    /// up the manipulation of the gene by reducing pressure on swap memory - we avoid
    /// pulling in the code segments to e.g. read/write PDDB.
    gene_cache: Option<Diploid>,
    /// This storage allows someone to "undo" a new pattern if they don't like it
    prior_gene: Option<Diploid>,
    /// tracks the dynamic rate as users try to increase it
    mutation_rate: MutationRate,
    /// snapshots the rate at a given point to make the protocol less frustrating
    /// (i.e. if there's some goof-ups or delays you don't lose the rate you "earned")
    final_rate: MutationRate,
    nonce_mine: Option<[u8; 12]>,
    previous_mode: Option<VaultMode>,
}

impl GlobalConfig {
    pub fn init() -> (Self, VaultMode) {
        let xns = xous_names::XousNames::new().unwrap();
        let keystore = keystore::Keystore::new(&xns);
        let is_developer = keystore.is_developer().expect("couldn't query developer mode");
        let pddb = pddb::Pddb::new();

        let mut flags = keystore.get_flags().expect("couldn't get flags");
        let was_cold_boot = !flags.warm_boot();
        if !was_cold_boot {
            log::debug!("***** warm boot detected *****");
        } else {
            log::debug!("===== cold boot detected =====");
        }
        // now set the warm boot field as true - so if we go into deep sleep we can detect that
        flags.set_warm_boot(true);
        keystore.set_flags(flags).unwrap();

        // initialize the system state from the PDDB
        let mut k0 = [0u8; 32]; // keep it secret, keep it safe!
        let k0_len = read_pddb(&pddb, DC34_SECRET, &mut k0);

        let mut skip_tour_buf = [0u8; 1];
        read_pddb(&pddb, DC34_TOUR, &mut skip_tour_buf);
        let skip_tour = skip_tour_buf[0] != 0;
        log::info!("skip_tour {:?},  {:x?}", skip_tour, skip_tour_buf);

        let mut skip_token_tour_buf = [0u8; 1];
        read_pddb(&pddb, DC34_TOKEN_TOUR, &mut skip_token_tour_buf);
        let skip_token_tour = skip_token_tour_buf[0] != 0;
        log::info!("skip_token_tour {:?},  {:x?}", skip_token_tour, skip_token_tour_buf);

        let conn = xns.request_connection_blocking(dc34_api::LED_SERVER).unwrap();

        let mut attach_state = AttachState::FactoryNew;
        let mut badge_code = [BadgeType::None as u8; 1];
        let badge_code_len = read_pddb(&pddb, DC34_BADGE, &mut badge_code);
        let stored_type = if badge_code_len != 0 {
            BadgeType::try_from(badge_code[0]).unwrap_or(BadgeType::None)
        } else {
            BadgeType::None
        };
        let badge_type_measured = read_badgetype_pins();
        // init_light_gene(BadgeType::Uber); // only used to reset a badge to Uber in case of a mating failure
        let badge_type = if badge_code_len == 0 {
            if badge_type_measured != BadgeType::None {
                attach_state = AttachState::FirstMate;
                init_light_gene(badge_type_measured);
            }
            // none stored, init from pins
            badge_type_measured
        } else {
            if stored_type == BadgeType::None {
                // if the stored type is None, check and see if something new has been mated
                if badge_type_measured != BadgeType::None {
                    attach_state = AttachState::FirstMate;
                    init_light_gene(badge_type_measured);
                }
                badge_type_measured
            } else {
                // just return the previously stored type, ignore the pins - so if the badge is re-mated, it
                // doesn't overwrite the factory-initialized type
                stored_type
            }
        };

        if attach_state != AttachState::FirstMate {
            if badge_type_measured == BadgeType::None {
                // we're not plugged into a badge
                if stored_type == BadgeType::None {
                    // stored type is None, no badge attached - factory new
                    if k0 == [0u8; 32] || k0_len != 32 {
                        attach_state = AttachState::FactoryNew;
                    } else {
                        attach_state = AttachState::TestedStandAlone;
                    }
                } else {
                    // stored type is initialized, but no badge attached - go to token mode
                    attach_state = AttachState::Unattached;
                }
            } else {
                // we're plugged into a badge
                if stored_type == badge_type_measured {
                    attach_state = AttachState::Matched;
                } else {
                    attach_state = AttachState::Mismatched;
                }
            }
        }
        log::info!(
            "badge type measured: {:?}, stored type: {:?}, attach_state: {:?}",
            badge_type_measured,
            stored_type,
            attach_state
        );

        // at this point, a light gene should be initialized, if the module has been
        // mated with a badge. Tell the led server to read the initial copy.
        // blocks until complete. Safe to call if no gene is on disk, falls back to
        // a `None` representation which puts up the test pattern.
        let gene = get_light_gene();
        if let Some(gene) = gene {
            gene.send(conn, LedManagerOp::SetGene.to_usize().unwrap());
        }

        let initial_mode = match attach_state {
            AttachState::FactoryNew => {
                if !is_developer {
                    VaultMode::FactoryTest
                } else {
                    VaultMode::IdleDevMode
                }
            }
            AttachState::FirstMate | AttachState::TestedStandAlone => VaultMode::Idle,
            AttachState::Matched | AttachState::Mismatched => {
                if skip_tour || !was_cold_boot {
                    if is_developer { VaultMode::IdleDevMode } else { VaultMode::Idle }
                } else {
                    VaultMode::Tour
                }
            }
            AttachState::Unattached => {
                if skip_token_tour || !was_cold_boot {
                    VaultMode::Password
                } else {
                    VaultMode::TokenTour
                }
            }
        };

        let power_server = xns.request_connection_blocking(dc34_api::POWER_MANAGER_SERVER).unwrap();

        (
            GlobalConfig {
                is_developer,
                attach_state,
                was_cold_boot,
                k0,
                skip_tour,
                skip_token_tour,
                badge_type,
                led_server: conn,
                power_server,
                display_fade_cache: false,
                gene_cache: gene,
                prior_gene: None,
                mutation_rate: MutationRate::Baseline,
                final_rate: MutationRate::Baseline,
                nonce_mine: None,
                previous_mode: None,
            },
            initial_mode,
        )
    }

    #[allow(dead_code)]
    pub fn skip_tour(&self) -> bool { self.skip_tour }

    #[allow(dead_code)]
    pub fn skip_token_tour(&self) -> bool { self.skip_token_tour }

    pub fn is_developer(&self) -> bool { self.is_developer }

    pub fn attachment_state(&self) -> AttachState { self.attach_state }

    #[allow(dead_code)]
    pub fn was_cold_boot(&self) -> bool { self.was_cold_boot }

    pub fn k0_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.k0);
        let digest: [u8; 32] = hasher.finalize().try_into().unwrap();
        let mut buffer = [0u8; 64];
        hex::encode_to_slice(digest, &mut buffer).unwrap();
        let hex_str = core::str::from_utf8(&buffer).unwrap();
        hex_str[..8].to_string()
    }

    pub fn update_power_state(&mut self, current_mode: VaultMode) {
        #[cfg(not(feature = "uber"))]
        const SHORT_TIMEOUT: usize = 36;
        #[cfg(not(feature = "uber"))]
        const MEDIUM_TIMEOUT: usize = 69;
        #[cfg(not(feature = "uber"))]
        const LONG_TIMEOUT: usize = 0x69;

        #[cfg(feature = "uber")]
        const SHORT_TIMEOUT: usize = 4 * 60 * 60; // four hours
        #[cfg(feature = "uber")]
        const MEDIUM_TIMEOUT: usize = 8 * 60 * 60;
        #[cfg(feature = "uber")]
        const LONG_TIMEOUT: usize = 8 * 60 * 60;
        // handles None case, as well as Some(previous_mode) not the same as Some(current_mode)
        if self.previous_mode != Some(current_mode) {
            let (enable, duration_sec) = match current_mode {
                VaultMode::About => (true, SHORT_TIMEOUT),
                VaultMode::ConfirmGene => (true, MEDIUM_TIMEOUT),
                VaultMode::FactoryTest => (false, 0),
                VaultMode::StandAloneTest => (false, 0),
                VaultMode::DefconHelp => (true, SHORT_TIMEOUT),
                VaultMode::TokenHelp => (true, MEDIUM_TIMEOUT),
                VaultMode::Idle => (true, SHORT_TIMEOUT),
                VaultMode::IdleDevMode => (true, SHORT_TIMEOUT),
                VaultMode::Password => (true, MEDIUM_TIMEOUT),
                VaultMode::Totp => (true, MEDIUM_TIMEOUT),
                VaultMode::GeneScan => (true, LONG_TIMEOUT),
                VaultMode::ResponseGene { quantum: _ } => (true, LONG_TIMEOUT),
                VaultMode::ShowKey { quantum: _ } => (true, LONG_TIMEOUT),
                VaultMode::TokenTour => (true, MEDIUM_TIMEOUT),
                VaultMode::Tour => (true, MEDIUM_TIMEOUT),
            };
            self.power_manager_config(enable, Some(duration_sec));
        }
        self.previous_mode = Some(current_mode);
    }

    pub fn set_skip_tour(&mut self, state: bool) -> VaultMode {
        let pddb = pddb::Pddb::new();
        let mut key = pddb
            .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
            .expect("couldn't get PDDB key");
        if state {
            key.write(&[1]).ok();
        } else {
            key.write(&[0]).ok();
        }
        if self.attach_state.attached() {
            if self.is_developer { VaultMode::IdleDevMode } else { VaultMode::Idle }
        } else {
            VaultMode::Password
        }
    }

    pub fn is_badge_attached(&self) -> bool { self.attach_state.attached() }

    pub fn badge_type(&self) -> BadgeType { self.badge_type }

    pub fn set_mutation_rate(&mut self, new_rate: MutationRate) { self.mutation_rate = new_rate; }

    pub fn lock_rate(&mut self) { self.final_rate = self.mutation_rate; }

    pub fn get_mutation_rate(&self) -> MutationRate { self.mutation_rate }

    pub fn get_final_rate(&self) -> MutationRate { self.final_rate }

    pub fn generate_my_nonce(&mut self) {
        loop {
            let mut nonce = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce);
            if nonce == DC34_HEADER[..12] {
                // generate a new one
                continue;
            }
            if let Some(prev) = self.nonce_mine {
                if prev != nonce {
                    self.nonce_mine = Some(nonce);
                    break;
                } else {
                    // generate a new one
                }
            } else {
                self.nonce_mine = Some(nonce);
                break;
            }
        }
    }

    pub fn get_my_nonce(&self) -> Option<Nonce> {
        if let Some(n) = self.nonce_mine { Some(Nonce::clone_from_slice(&n)) } else { None }
    }

    pub fn get_padded_gamete(&self) -> Option<[u8; 16]> {
        if let Some(gene) = self.gene_cache {
            let mut d = [0u8; 16];
            let mut gamete = gene.meiosis();
            mutate(&mut gamete, self.final_rate);
            let serialized = gamete.serialize();
            let len = serialized.len().min(15); // save last byte for badge type
            d[..len].copy_from_slice(&serialized[..len]);
            d[15] = self.badge_type as u8;
            Some(d)
        } else {
            None
        }
    }

    pub fn get_egg(&self, rate: Option<MutationRate>) -> Option<Haploid> {
        if let Some(gene) = self.gene_cache {
            let mut gamete = gene.meiosis();
            // pick the larger of the internal rate or the passed-in rate
            mutate(&mut gamete, rate.unwrap_or(self.final_rate).max(self.final_rate));
            Some(gamete)
        } else {
            None
        }
    }

    // this automatically copies the old gene to the backup location
    pub fn replace_gene(&mut self, egg: Haploid, sperm: Haploid) {
        self.prior_gene = self.gene_cache.take();
        self.gene_cache = Some(Diploid([egg, sperm]))
    }

    pub fn revert_gene(&mut self) {
        if let Some(backup_gene) = self.prior_gene.take() {
            self.gene_cache = Some(backup_gene);
        }
    }

    pub fn gene(&self) -> Option<Diploid> { self.gene_cache }

    pub fn render_gene(&self) {
        if let Some(gene) = self.gene_cache {
            gene.send(self.led_server, LedManagerOp::SetGene.to_usize().unwrap());
        }
    }

    pub fn nonce_data(&mut self) -> Vec<u8> {
        self.generate_my_nonce();
        [DC34_HEADER.as_slice(), self.nonce_mine.unwrap().as_slice()].concat()
    }

    pub fn cipher(&self) -> Aes256GcmSiv { Aes256GcmSiv::new((&self.k0).into()) }

    pub fn clear_nonces(&mut self) { self.nonce_mine.take(); }

    pub fn display_fading(&mut self, enable: bool) {
        if self.display_fade_cache != enable {
            log::debug!("setting fade: {:?}", enable);
            xous::send_message(
                self.power_server,
                xous::Message::new_scalar(
                    PowerManagerOp::SetFadeMode.to_usize().unwrap(),
                    if enable { 1 } else { 0 },
                    0,
                    0,
                    0,
                ),
            )
            .ok();
            self.display_fade_cache = enable;
        }
    }

    pub fn power_manager_config(&self, enable: bool, duration_sec: Option<usize>) {
        xous::send_message(
            self.power_server,
            xous::Message::new_blocking_scalar(
                PowerManagerOp::Enable.to_usize().unwrap(),
                if enable { 1 } else { 0 },
                duration_sec.unwrap_or(0),
                0,
                0,
            ),
        )
        .ok();
    }

    pub fn is_plugged_in(&self) -> bool {
        match xous::send_message(
            self.power_server,
            xous::Message::new_blocking_scalar(PowerManagerOp::GetVbus.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(_op, _ok, vbus, _, _)) => vbus != 0,
            _ => unimplemented!("Unhandled internal error"),
        }
    }

    // notify the power server that we've booted - this enables all the power management routines
    pub fn power_manager_boot_finished(&self) {
        xous::send_message(
            self.power_server,
            xous::Message::new_blocking_scalar(PowerManagerOp::Boot.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    pub fn power_off(&self) {
        xous::send_message(
            self.power_server,
            xous::Message::new_scalar(PowerManagerOp::PowerOff.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    pub fn screen_off(&self) {
        xous::send_message(
            self.power_server,
            xous::Message::new_scalar(PowerManagerOp::ScreenOffRequest.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .ok();
    }

    pub fn pause_accel(&self, pause: bool) {
        xous::send_message(
            self.power_server,
            xous::Message::new_blocking_scalar(
                PowerManagerOp::PauseAccel.to_usize().unwrap(),
                if pause { 1 } else { 0 },
                0,
                0,
                0,
            ),
        )
        .ok();
    }
}

// GlobalConfig *must* be thread-safe. Don't add stuff to it that's not Send + Sync
#[allow(dead_code)]
trait AssertSendSync: Send + Sync {}
impl AssertSendSync for GlobalConfig {}

pub fn read_pddb(pddb: &Pddb, key: &str, buf: &mut [u8]) -> usize {
    let mut key = pddb
        .get(DC34_DICT, key, None, true, true, Some(buf.len()), None::<fn()>)
        .expect("couldn't get PDDB key");
    key.read(buf).expect("couldn't read key")
}

/// The side-effect call allows us to set global mutable state on disk
/// without having to actually share the GlobalConfig object
pub fn side_effect_skip_token_tour(state: bool) {
    // disable repeating the tour - show it only once
    let pddb = pddb::Pddb::new();
    let mut key = pddb
        .get(
            crate::config::DC34_DICT,
            crate::config::DC34_TOKEN_TOUR,
            None,
            true,
            true,
            Some(1),
            None::<fn()>,
        )
        .expect("couldn't get PDDB key");
    if state {
        key.write(&[1]).ok();
    } else {
        key.write(&[0]).ok();
    }
}

pub fn read_badgetype_pins() -> BadgeType {
    let iox = bao1x_api::iox::IoxHal::new();
    for &(port, pin) in SAO_GPIO[..3].iter() {
        iox.setup_pin(
            port,
            pin,
            Some(bao1x_api::IoxDir::Input),
            Some(bao1x_api::IoxFunction::Gpio),
            Some(bao1x_api::IoxEnable::Enable),
            Some(bao1x_api::IoxEnable::Enable),
            None,
            None,
        );
    }
    let mut bits: u8 = 0;
    // few milliseconds for the pull-ups to do their magic
    std::thread::sleep(std::time::Duration::from_millis(5));
    for (i, &(port, pin)) in SAO_GPIO[..3].iter().enumerate() {
        if iox.get_gpio_pin_value(port, pin) == IoxValue::High {
            bits |= 1 << (i as u8);
        }
    }
    BadgeType::try_from(bits).unwrap_or(BadgeType::None)
}
