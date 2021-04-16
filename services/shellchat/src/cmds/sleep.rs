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

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        let mut ret = String::<1024>::new();
        let helpstring = "sleep [now] [current] [hard]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "now" => {
                    if ((env.llio.adc_vbus().unwrap() as f64) * 0.005033) > 1.5 {
                        // if power is plugged in, deny powerdown request
                        write!(ret, "System can't sleep while charging. Unplug charging cable and try again.").unwrap();
                    } else {
                        if Ok(true) == env.gam.powerdown_request() {
                            env.ticktimer.sleep_ms(500).unwrap(); // let the screen redraw

                            // allow EC to snoop, so that it can wake up the system
                            env.llio.allow_ec_snoop(true).unwrap();
                            // allow the EC to power me down
                            env.llio.allow_power_off(true).unwrap();
                            // now send the power off command
                            env.com.power_off_soc().unwrap();

                            log::info!("CMD: powering down now!");
                            // pause execution, nothing after this should be reachable
                            env.ticktimer.sleep_ms(2000).unwrap(); // should power off within 2 seconds
                            log::info!("CMD: if you can read this, power down failed!");
                        }
                        write!(ret, "Powerdown request denied").unwrap();
                    }
                }
                "current" => {
                    if let Some(i) = env.com.get_standby_current().unwrap() {
                        write!(ret, "Last standby current was {}mA", i).unwrap();
                    } else {
                        write!(ret, "Standby current measurement not initialized.").unwrap();
                    }
                }
                "hard" => {
                    write!(ret, "Hard shutdown not yet implemented").unwrap();
                }
                _ =>  write!(ret, "{}", helpstring).unwrap(),
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
