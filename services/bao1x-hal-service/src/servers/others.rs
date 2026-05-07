//! Server for all the other hardware pieces that aren't covered by
//! existing well-known/shared service providers, and which also can't
//! be condensed into the main loop due to dependencies on core HAL
//! services (such as IFRAM resolution, I/O setup, etc.)

use bao1x_api::{IoxDir, IoxEnable, IoxFunction, PeripheralOpcode, SERVER_NAME_BAO1X_OTHERS, iox::IoSetup};
use bao1x_hal::udma::{Adc, AdcExtChannel, AdcSource};
use bao1x_hal_service::UdmaGlobal;

pub fn start_peri_service() {
    let _ = std::thread::spawn({
        move || {
            peri_service();
        }
    });
}

fn peri_service() -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    // claim the server name
    let sid = xns.register_name(SERVER_NAME_BAO1X_OTHERS, None).unwrap();

    let iox = crate::iox::IoxHal::new();
    let udma_global = UdmaGlobal::new();
    udma_global.udma_clock_config(bao1x_api::PeriphId::Adc, true);
    let clk_mgr = bao1x_hal_service::ClockManager::new();
    // safety: clocks are turned on, and perclk already configured as so
    let mut adc = unsafe { Adc::new(clk_mgr.get_per()) };

    let mut msg_opt = None;

    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(PeripheralOpcode::InvalidCall)
        };
        log::debug!("{:?}", opcode);
        match opcode {
            PeripheralOpcode::ReadAdcChannel => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let channel = AdcSource::from_usize(scalar.arg1);
                    // sets to at least 1, even if the arg is 0
                    // max averaging is limited by the size of the ADC buffer
                    let averaging =
                        scalar.arg2.max(1).min(bao1x_hal::udma::ADC_RX_BUF_SIZE / size_of::<u32>());
                    scalar.arg1 = adc.read_raw_averaged(channel, averaging) as usize;
                }
            }
            PeripheralOpcode::EnableChannel => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let channel: AdcExtChannel = scalar.arg1.into();
                    let pin = match channel {
                        AdcExtChannel::Adc0 => 4,
                        AdcExtChannel::Adc1 => 5,
                        AdcExtChannel::Adc2 => 6,
                        AdcExtChannel::Adc3 => 7,
                    };
                    iox.setup_pin(
                        bao1x_api::IoxPort::PA,
                        pin,
                        Some(IoxDir::Input),
                        Some(IoxFunction::Gpio),
                        Some(IoxEnable::Disable),
                        Some(IoxEnable::Disable),
                        None,
                        None,
                    );
                    // dabao boards have a dependency to configure this pin as an input as well.
                    #[cfg(feature = "board-dabao")]
                    if channel == AdcExtChannel::Adc0 {
                        iox.setup_pin(
                            bao1x_api::IoxPort::PC,
                            9,
                            Some(IoxDir::Input),
                            Some(IoxFunction::Gpio),
                            Some(IoxEnable::Disable),
                            Some(IoxEnable::Disable),
                            None,
                            None,
                        );
                    }
                }
            }
            PeripheralOpcode::UpdatePerclk => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let new_perclk = scalar.arg1 as u32;
                    adc.update_perclk(new_perclk);
                }
            }
            PeripheralOpcode::InvalidCall => {
                log::error!("Invalid opcode received: {:?}", msg_opt);
            }
        }
    }
}
