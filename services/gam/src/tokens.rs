use xous_ipc::String;

/*
    Authentication tokens to the GAM are created on a first-come, first-serve basis,
    under the following assumptions:
       - the boot set is fully trusted (signature checked, just-my-own-code running)
       - the boot set will grab all the token slots availble before allowing any less-trusted code to run

    This scheme thus effectively locks out less-trusted code, while simplifying the
    registration of interprocess comms between trusted elements, only relying on ephemeral,
    dynamically generated 128-bit tokens.
*/
const TOKEN_SLOTS: usize = 3;
#[derive(Copy, Clone, Debug)]
pub(crate) struct NamedToken {
    token: [u32; 4],
    name: String::<128>,
}
pub(crate) struct TokenManager {
    tokens: [Option<NamedToken>; TOKEN_SLOTS],
    slot_names: [&'static str; TOKEN_SLOTS],
    trng: trng::Trng,
}
impl<'a> TokenManager {
    pub(crate) fn new(xns: &xous_names::XousNames) -> TokenManager {
        TokenManager {
            tokens: [None; TOKEN_SLOTS],
            slot_names: ["status", "menu", "passwords"],
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
        allow
    }
    pub(crate) fn claim_token(&mut self, name: &str) -> Option<[u32; 4]> {
        // first check if the name is valid
        let mut valid = false;
        for &valid_name in self.slot_names.iter() {
            if name.eq(valid_name) {
                valid = true;
            }
        }
        if !valid { return None }
        // now check if it hasn't already been registered
        let mut registered = false;
        for maybe_token in self.tokens.iter() {
            match maybe_token {
                Some(token) => {
                    if name.eq(token.name.as_str().unwrap()) {
                        registered = true;
                    }
                }
                _ => ()
            }
        }
        if registered { return None }
        // now do the registration
        let token = [self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(), self.trng.get_u32().unwrap(),];
        for maybe_token in self.tokens.iter_mut() {
            if maybe_token.is_none() {
                *maybe_token = Some(NamedToken {
                    token,
                    name: String::<128>::from_str(name),
                });
            }
            return Some(token)
        }
        // somehow, we didn't have space -- but with all the previous checks, we really should have
        None
    }
    pub(crate) fn is_token_valid(&self, token: [u32; 4]) -> bool {
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
}
