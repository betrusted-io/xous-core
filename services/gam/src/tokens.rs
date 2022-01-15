/*
    Authentication tokens to the GAM are created on a first-come, first-serve basis,
    under the following assumptions:
       - the boot set is fully trusted (signature checked, just-my-own-code running)
       - the boot set will grab all the token slots availble before allowing any less-trusted code to run

    This scheme thus effectively locks out less-trusted code, while simplifying the
    registration of interprocess comms between trusted elements, only relying on ephemeral,
    dynamically generated 128-bit tokens.
*/

// if you add more UxContexts, and you want them authorized by the GAM, add their names here.
const TOKEN_SLOTS: usize = 9;
const EXPECTED_BOOT_CONTEXTS: [&'static str; TOKEN_SLOTS] = [
    "shellchat",
    "main menu",
    "status",
    "emoji menu",
    "rootkeys modal",
    "rootkeys menu",
    "pddb modal",
    "shared modal",
    "pddb menu",
    //"user app menu",
];

#[derive(Clone, Debug)]
pub(crate) struct NamedToken {
    token: [u32; 4],
    name: String,
}
pub(crate) struct TokenManager {
    tokens: [Option<NamedToken>; TOKEN_SLOTS],
    trng: trng::Trng,
}
impl<'a> TokenManager {
    pub(crate) fn new(xns: &xous_names::XousNames) -> TokenManager {
        TokenManager {
            tokens: Default::default(),
            trng: trng::Trng::new(&xns).unwrap(),
        }
    }
    /// checks to see if all the slots have been occupied. We can't allow untrusted code to run until all slots have checked in
    pub(crate) fn allow_untrusted_code(&self) -> bool {
        let mut allow = true;
        for t in self.tokens.iter() {
            if t.is_none() {
                allow = false
            }
        }
        // throw a bone to the dev who has to debug this error. This typically only triggers after a major
        // refactor and some UX element was removed and we forgot to update it in this table here.
        if !allow {
            log::info!("Occupied token slots:");
            for t in self.tokens.iter() {
                if let Some(s) = t {
                    log::info!("{}", s.name);
                }
            }
        }
        allow
    }
    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        log::trace!("claiming token {}", name);
        // first check if the name is valid
        let mut valid = false;
        for &valid_name in EXPECTED_BOOT_CONTEXTS.iter() {
            if name.eq(valid_name) {
                valid = true;
            }
        }
        if !valid {
            log::error!("Server {} is not pre-registered in gam/src/tokens.rs. Did you forget to register it?", name);
            return None
        }
        // now check if it hasn't already been registered
        let mut registered = false;
        for maybe_token in self.tokens.iter() {
            match maybe_token {
                Some(token) => {
                    if name.eq(token.name.as_str()) {
                        registered = true;
                    }
                }
                _ => ()
            }
        }
        if registered {
            log::error!("Attempt to re-register a UX context: {}", name);
            return None
        }
        // now do the registration
        let token = [self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(),];
        log::trace!("registering {} to {:x?}", name, token);
        for maybe_token in self.tokens.iter_mut() {
            if maybe_token.is_none() {
                *maybe_token = Some(NamedToken {
                    token,
                    name: String::from(name),
                });
                log::trace!("token table after registration is {:x?}", self.tokens);
                return Some(token)
            }
        }
        // somehow, we didn't have space -- but with all the previous checks, we really should have
        None
    }
    pub(crate) fn is_token_valid(&self, token: [u32; 4]) -> bool {
        log::trace!("checking for validity of token {:x?}", token);
        log::trace!("token table is {:x?}", self.tokens);
        for maybe_token in self.tokens.iter() {
            match maybe_token {
                Some(found_token) => {
                    if found_token.token == token {
                        return true
                    }
                }
                _ => ()
            }
        }
        false
    }
    pub(crate) fn find_token(&self, name: &str) -> Option<[u32; 4]> {
        for maybe_token in self.tokens.iter() {
            if let Some(token) = maybe_token {
                if token.name == String::from(name) {
                    return Some(token.token)
                }
            }
        }
        None
    }
}
