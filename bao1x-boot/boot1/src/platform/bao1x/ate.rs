use bao1x_hal::udma::{AdcExtChannel, AdcSource, GlobalConfig};

#[repr(C)]
pub struct Ate {
    adc: [[u16; 5]; 3],
}
impl Ate {
    pub fn new(perclk: u32) -> Ate {
        use bao1x_api::{IoGpio, IoSetup};
        use bao1x_hal::iox::Iox;
        let iox = Iox::new(utralib::utra::iox::HW_IOX_BASE as *mut u32);
        let udma_global = GlobalConfig::new();
        udma_global.clock_on(bao1x_api::PeriphId::Adc);
        // safety: clocks have been turned on. The ADC buffer is located at the base of IFRAM0 which should
        // be empty, as IFRAM reserved addresses allocate from top-down.
        let mut adc = unsafe { bao1x_hal::udma::Adc::new_baremetal(perclk, utralib::HW_IFRAM0_MEM) };
        // setup PF1 as an "index" pin
        iox.setup_pin(
            bao1x_api::IoxPort::PF,
            1,
            Some(bao1x_api::IoxDir::Output),
            Some(bao1x_api::IoxFunction::Gpio),
            None,
            Some(bao1x_api::IoxEnable::Disable),
            None,
            None,
        );
        // bring the pin low to indicate that we're starting the test
        iox.set_gpio_pin_value(bao1x_api::IoxPort::PF, 1, bao1x_api::IoxValue::Low);
        // setup PA4..=PA7 as inputs - just to make sure we aren't accidentally driving them.
        // disable the pull-up, too.
        for pin in 4..=7 {
            iox.setup_pin(
                bao1x_api::IoxPort::PA,
                pin,
                Some(bao1x_api::IoxDir::Input),
                Some(bao1x_api::IoxFunction::Gpio),
                Some(bao1x_api::IoxEnable::Disable),
                Some(bao1x_api::IoxEnable::Disable),
                None,
                None,
            );
        }
        // pipe-clear any stale ADC values
        let _dummy = adc.read_raw_averaged(AdcSource::Ext(AdcExtChannel::Adc0), 8);

        let sources = [
            AdcSource::Temperature,
            AdcSource::Ext(AdcExtChannel::Adc0),
            AdcSource::Ext(AdcExtChannel::Adc1),
            AdcSource::Ext(AdcExtChannel::Adc2),
            AdcSource::Ext(AdcExtChannel::Adc3),
        ];
        let mut ate: Ate = Ate { adc: [[0xDEADu16; 5]; 3] };
        for sample in 0..3 {
            // 0->1 on PF1 indicates zone for updating analog values within 5ms
            iox.set_gpio_pin_value(bao1x_api::IoxPort::PF, 1, bao1x_api::IoxValue::High);
            crate::platform::delay(5);
            iox.set_gpio_pin_value(bao1x_api::IoxPort::PF, 1, bao1x_api::IoxValue::Low);
            // 1->0 on PF1 indicates sampling values
            for (channel, source) in sources.iter().enumerate() {
                let _dummy = adc.read_raw_averaged(*source, 8); // dummy reading still required every source change, some bug in ADC driver?
                let raw = adc.read_raw_averaged(*source, 8);
                ate.adc[sample][channel] = raw;
                /* // don't include this in production code, it adds a lot of overhead & time!
                crate::println!(
                    "sample {} source {:?}: {} {} {}",
                    sample,
                    source,
                    raw,
                    bao1x_hal::udma::Adc::raw_to_temp_celsius(raw),
                    bao1x_hal::udma::Adc::raw_to_voltage(raw)
                );
                */
            }
            crate::platform::delay(5);
        }
        ate
    }

    pub fn serialize_into(&self, buf: &mut [u8; 32]) {
        let mut idx = 0usize;
        for sample_set in self.adc.iter() {
            for &adc_data in sample_set.iter() {
                let b = adc_data.to_le_bytes();
                buf[idx] = b[0];
                buf[idx + 1] = b[1];
                idx += 2;
            }
        }
    }
}
