use crate::{ShellCmdApi,CommonEnv};
use xous::String;

#[derive(Debug)]
pub struct Sleep {
}
impl Sleep {
    pub fn new() -> Self {
        Sleep {}
    }
}

const VERB: &str = "sleep";

impl<'a> ShellCmdApi<'a> for Sleep {
    fn process(&mut self, _rest: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        // TODO: check if power is plugged in first, we can't sleep when plugged in

        if Ok(true) == gam::powerdown_request(env.gam) {
            llio::allow_power_off(env.llio, true).unwrap();
            com::power_off_soc(env.com).unwrap();

            // pause execution, nothing after this should be reachable
            ticktimer_server::sleep_ms(env.ticktimer, 2000).unwrap(); // should power off within 2 seconds
        }

        let mut ret = String::<1024>::new();
        write!(ret, "Powerdown request denied").unwrap();
        Ok(Some(ret))
    }


    fn verb(&self) -> &'static str {
        VERB
    }
    fn matches(&self, verb: &str) -> bool {
        if verb == VERB {
            true
        } else {
            false
        }
    }
}