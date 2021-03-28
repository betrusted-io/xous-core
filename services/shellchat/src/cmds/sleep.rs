use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct Sleep {
}
impl Sleep {
    pub fn new() -> Self {
        Sleep {}
    }
}

impl<'a> ShellCmdApi<'a> for Sleep {
    cmd_api!(sleep); // inserts boilerplate for command API

    fn process(&mut self, _args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        let mut ret = String::<1024>::new();
        if ((llio::adc_vbus(env.llio).unwrap() as f64) * 0.005033) > 1.5 {
            // if power is plugged in, deny powerdown request
            write!(ret, "System can't sleep while charging. Unplug charging cable and try again.").unwrap();
            Ok(Some(ret))
        } else {
            if Ok(true) == gam::powerdown_request(env.gam) {
                ticktimer.sleep_ms(env.ticktimer, 500).unwrap(); // let the screen redraw

                // allow EC to snoop, so that it can wake up the system
                llio::allow_ec_snoop(env.llio, true).unwrap();
                // allow the EC to power me down
                llio::allow_power_off(env.llio, true).unwrap();
                // now send the power off command
                com::power_off_soc(env.com).unwrap();

                log::info!("CMD: powering down now!");
                // pause execution, nothing after this should be reachable
                ticktimer.sleep_ms(env.ticktimer, 2000).unwrap(); // should power off within 2 seconds
                log::info!("CMD: if you can read this, power down failed!");
            }
            write!(ret, "Powerdown request denied").unwrap();
            Ok(Some(ret))
        }
    }
}
