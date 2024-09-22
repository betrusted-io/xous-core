use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Sensors {}
impl Sensors {
    pub fn new() -> Self { Sensors {} }
}

impl<'a> ShellCmdApi<'a> for Sensors {
    cmd_api!(sensors);

    fn process(
        &mut self,
        _args: String,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        let milli_celcius = (((env.llio.adc_temperature().unwrap() as u32) * 12304) - 27_315_000) / 100;
        write!(
            ret,
            "Vbus {:.2}mV\nVint {:.2}mV\nVaux {:.2}mV\nVbram {:.2}mV\nUSB {:.2}|{:.2}mV\nTemp {}.{}°C",
            ((env.llio.adc_vbus().unwrap() as u32) * 503) / 100,
            ((env.llio.adc_vccint().unwrap() as u32) * 1000) / 1365,
            ((env.llio.adc_vccaux().unwrap() as u32) * 1000) / 1365,
            ((env.llio.adc_vccbram().unwrap() as u32) * 1000) / 1365,
            ((env.llio.adc_usb_p().unwrap() as u32) * 1000) / 1365,
            ((env.llio.adc_usb_n().unwrap() as u32) * 1000) / 1365,
            milli_celcius / 1000,
            (milli_celcius % 1000) / 100 // 1 decimal extra
        )
        .unwrap();

        Ok(Some(ret))
    }
}
