use gam::EXPECTED_BOOT_CONTEXTS;

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
    tokens: Vec::<NamedToken>,
    trng: trng::Trng,
}
impl<'a> TokenManager {
    pub(crate) fn new(xns: &xous_names::XousNames) -> TokenManager {
        TokenManager {
            tokens: Vec::new(),
            trng: trng::Trng::new(&xns).unwrap(),
        }
    }
    /// checks to see if all the slots have been occupied. We can't allow untrusted code to run until all slots have checked in
    pub(crate) fn allow_untrusted_code(&self) -> bool {
        if self.tokens.len() == EXPECTED_BOOT_CONTEXTS.len() {
            true
        } else {
            // throw a bone to the dev who has to debug this error. This typically only triggers after a major
            // refactor and some UX element was removed and we forgot to update it in this table here.
            log::info!("Occupied token slots:");
            for t in self.tokens.iter() {
                log::info!("{}", t.name);
            }
            false
        }
    }
    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        log::trace!("claiming token {}", name);
        // first check if the name is valid
        if EXPECTED_BOOT_CONTEXTS.iter().find(|&&context| context == name).is_none() {
            log::error!("Server {} is not pre-registered in gam/lib.rs/EXPECTED_BOOT_CONTEXTS. Did you forget to register it?", name);
            return None
        }
        // now check if it hasn't already been registered
        if self.tokens.iter().find(|&namedtoken| namedtoken.name == name).is_some() {
            log::error!("Attempt to re-register a UX context: {}", name);
            return None
        }
        // now do the registration
        let token = [self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(),];
        log::trace!("registering {} to {:x?}", name, token);
        self.tokens.push(
            NamedToken {
                token,
                name: String::from(name),
            }
        );
        return Some(token)
    }
    pub(crate) fn is_token_valid(&self, token: [u32; 4]) -> bool {
        log::trace!("checking for validity of token {:x?}", token);
        log::trace!("token table is {:x?}", self.tokens);
        self.tokens.iter().find(|&namedtoken| namedtoken.token == token).is_some()
    }
    pub(crate) fn find_token(&self, name: &str) -> Option<[u32; 4]> {
        if let Some(i) = self.tokens.iter().position(|namedtoken| namedtoken.name == name) {
            Some(self.tokens[i].token)
        } else {
            None
        }
    }
}
