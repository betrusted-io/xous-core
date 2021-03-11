use crate::{ShellCmdApi,CommonEnv};
use xous::String;

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
           (llio::adc_vbus(env.llio).unwrap() as f64) * 0.005033,
           (llio::adc_vccint(env.llio).unwrap() as f64) / 1365.0,
           (llio::adc_vccaux(env.llio).unwrap() as f64) / 1365.0,
           (llio::adc_vccbram(env.llio).unwrap() as f64) / 1365.0,
           (llio::adc_usb_p(env.llio).unwrap() as f64) / 1365.0,
           (llio::adc_usb_n(env.llio).unwrap() as f64) / 1365.0,
           ((llio::adc_temperature(env.llio).unwrap() as f64) * 0.12304) - 273.15,
        ).unwrap();

        Ok(Some(ret))
    }
}