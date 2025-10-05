use std::cell::RefCell;

#[cfg(feature = "bao1x")]
use bao1x_hal_service::trng;
use gam::{EXPECTED_APP_CONTEXTS, EXPECTED_BOOT_CONTEXTS};

/*
    Authentication tokens to the GAM are created on a first-come, first-serve basis,
    under the following assumptions:
       - the boot set is fully trusted (signature checked, just-my-own-code running)
       - the boot set will grab all the token slots availble before allowing any less-trusted code to run

    This scheme thus effectively locks out less-trusted code, while simplifying the
    registration of interprocess comms between trusted elements, only relying on ephemeral,
    dynamically generated 128-bit tokens.
*/

#[derive(Clone, Debug)]
pub(crate) struct NamedToken {
    token: [u32; 4],
    name: String,
}
pub(crate) struct TokenManager {
    tokens: Vec<NamedToken>,
    #[cfg(feature = "unsafe-app-loading")]
    extra_names: Vec<String>,
    trng: trng::Trng,
    tt: ticktimer_server::Ticktimer,
    last_time: RefCell<u64>,
}
const REPEAT_MSG_INTERVAL_MS: u64 = 5000;

impl<'a> TokenManager {
    pub(crate) fn new(xns: &xous_names::XousNames) -> TokenManager {
        TokenManager {
            tokens: Vec::new(),
            #[cfg(feature = "unsafe-app-loading")]
            extra_names: Vec::new(),
            trng: trng::Trng::new(&xns).unwrap(),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            last_time: RefCell::new(0),
        }
    }

    /// checks to see if all the slots have been occupied. We can't allow untrusted code to run until all
    /// slots have checked in
    pub(crate) fn allow_untrusted_code(&self) -> bool {
        #[cfg(feature = "unsafe-app-loading")]
        let expected_len =
            EXPECTED_BOOT_CONTEXTS.len() + EXPECTED_APP_CONTEXTS.len() + self.extra_names.len();
        #[cfg(not(feature = "unsafe-app-loading"))]
        let expected_len = EXPECTED_BOOT_CONTEXTS.len() + EXPECTED_APP_CONTEXTS.len();
        if self.tokens.len() == expected_len {
            true
        } else {
            // throw a bone to the dev who has to debug this error. This typically only triggers after a major
            // refactor and some UX element was removed and we forgot to update it in this table here.
            let now = self.tt.elapsed_ms();
            if *self.last_time.borrow() + REPEAT_MSG_INTERVAL_MS < now {
                log::info!("Occupied token slots: ***");
                for t in self.tokens.iter() {
                    log::info!("  {}", t.name);
                }
                log::info!("Expected token slots:");
                for t in EXPECTED_BOOT_CONTEXTS.iter() {
                    log::info!("  {}", t);
                }
                for t in EXPECTED_APP_CONTEXTS.iter() {
                    log::info!("  {}", t);
                }
                #[cfg(feature = "unsafe-app-loading")]
                for t in self.extra_names.iter() {
                    log::info!("{}", t);
                }
                self.last_time.replace(now);
            }
            false
        }
    }

    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        log::trace!("claiming token {}", name);
        // first check if the name is valid
        let mut found = false;
        if EXPECTED_BOOT_CONTEXTS.iter().find(|&&context| context == name).is_some() {
            found = true;
        }
        if EXPECTED_APP_CONTEXTS.iter().find(|&&context| context == name).is_some() {
            found = true;
        }
        #[cfg(feature = "unsafe-app-loading")]
        if self.extra_names.iter().find(|&context| context == name).is_some() {
            found = true;
        }
        if !found {
            log::error!(
                "Server {} is not pre-registered in gam/lib.rs/EXPECTED_BOOT_CONTEXTS or apps.rs/EXPECTED_APP_CONTEXTS. Did you forget to register it?",
                name
            );
            return None;
        }
        // now check if it hasn't already been registered
        if self.tokens.iter().find(|&namedtoken| namedtoken.name == name).is_some() {
            log::error!("Attempt to re-register a UX context: {}", name);
            return None;
        }
        // now do the registration
        let token = [
            self.trng.get_u32().unwrap(),
            self.trng.get_u32().unwrap(),
            self.trng.get_u32().unwrap(),
            self.trng.get_u32().unwrap(),
        ];
        log::trace!("registering {} to {:x?}", name, token);
        self.tokens.push(NamedToken { token, name: String::from(name) });
        return Some(token);
    }

    pub(crate) fn is_token_valid(&self, token: [u32; 4]) -> bool {
        self.tokens.iter().find(|&namedtoken| namedtoken.token == token).is_some()
    }

    pub(crate) fn find_token(&self, name: &str) -> Option<[u32; 4]> {
        if let Some(i) = self.tokens.iter().position(|namedtoken| namedtoken.name == name) {
            log::debug!("found {}:{:?}", name, self.tokens[i].token);
            Some(self.tokens[i].token)
        } else {
            None
        }
    }

    pub(crate) fn lookup_name(&self, token: &[u32; 4]) -> Option<String> {
        for entry in self.tokens.iter() {
            if entry.token == *token {
                return Some(entry.name.to_string());
            }
        }
        None
    }

    /// Register a new name that can then claim a token. Note that only pre-registered applications are
    /// allowed to do this.
    #[cfg(feature = "unsafe-app-loading")]
    pub(crate) fn register_name(&mut self, name: &str, auth_token: &[u32; 4]) {
        if let Some(registrant) = self.lookup_name(auth_token) {
            if EXPECTED_BOOT_CONTEXTS.iter().find(|&&context| context == registrant).is_some()
                || EXPECTED_APP_CONTEXTS.iter().find(|&&context| context == registrant).is_some()
            {
                self.extra_names.push(name.to_string());
            } else {
                log::error!(
                    "`{}' does not have permission to register a new name because it is not pre-registered",
                    registrant
                );
            }
        } else {
            log::error!("Token {:?} does not correspond with a name", auth_token);
        }
    }
}
