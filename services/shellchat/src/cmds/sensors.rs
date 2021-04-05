use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct Sensors {
}
impl Sensors {
    pub fn new() -> Self {
        Sensors {}
    }
}


impl<'a> ShellCmdApi<'a> for Sensors {
    cmd_api!(sensors);

    fn process(&mut self, _args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();

        write!(ret, "Vbus {:.2}V\nVint {:.2}V\nVaux {:.2}V\nVbram {:.2}V\nUSB {:.2}|{:.2}V\nTemp {:.1}°C",
           (env.llio.adc_vbus().unwrap() as f64) * 0.005033,
           (env.llio.adc_vccint().unwrap() as f64) / 1365.0,
           (env.llio.adc_vccaux().unwrap() as f64) / 1365.0,
           (env.llio.adc_vccbram().unwrap() as f64) / 1365.0,
           (env.llio.adc_usb_p().unwrap() as f64) / 1365.0,
           (env.llio.adc_usb_n().unwrap() as f64) / 1365.0,
           ((env.llio.adc_temperature().unwrap() as f64) * 0.12304) - 273.15,
        ).unwrap();

        Ok(Some(ret))
    }
}
