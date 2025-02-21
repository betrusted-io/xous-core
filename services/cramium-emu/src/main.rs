mod keyboard;

use cramium_api::*;

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(cramium_api::SERVER_NAME_CRAM_HAL, None).expect("can't register server");

    // start keyboard emulator service
    keyboard::start_keyboard_service();

    let mut msg_opt = None;
    log::debug!("Starting main loop");
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode =
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(api::HalOpcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            HalOpcode::MapIfram => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let requested_size = scalar.arg1; // requested size
                    let _requested_bank = scalar.arg2; // Specifies bank 0, 1, or don't care (any number but 0 or 1)

                    // return as if the mapping passed
                    scalar.arg1 = requested_size;
                    scalar.arg2 = 0xDEAD_BEEF; // fake address
                }
            }
            HalOpcode::UnmapIfram => {
                if let Some(_scalar) = msg.body.scalar_message() {
                    // do nothing
                }
            }
            HalOpcode::ConfigureIox => {
                let buf =
                    unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let config = buf.to_original::<IoxConfigMessage, _>().unwrap();
                log::info!("Emulation got IO config request: {:?}", config);
            }
            HalOpcode::ConfigureIoxIrq => {
                let buf =
                    unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let registration = buf.to_original::<IoxIrqRegistration, _>().unwrap();
                log::info!("Got registration request: {:?}", registration);
            }
            HalOpcode::IrqLocalHandler => {}
            HalOpcode::SetGpioBank => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let port: IoxPort = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let value = scalar.arg2 as u16;
                    let bitmask = scalar.arg3 as u16;
                    log::info!("Set Gpio{:?}, {:x}&{:x}", port, value, bitmask);
                }
            }
            HalOpcode::GetGpioBank => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let port: IoxPort = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    log::info!("Get Gpio{:?} - returning 0", port);
                    scalar.arg1 = 0;
                }
            }
            HalOpcode::ConfigureUdmaClock => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let enable = if scalar.arg2 != 0 { true } else { false };
                    log::info!("Udma clock setting of {:?} for {:?}", enable, periph);
                }
            }
            HalOpcode::ConfigureUdmaEvent => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    let event_offset = scalar.arg2 as u32;
                    let to_channel: EventChannel =
                        num_traits::FromPrimitive::from_usize(scalar.arg3).unwrap();
                    log::info!("Udma configure event: {:?}/{:?}, {:x}", periph, to_channel, event_offset);
                }
            }
            HalOpcode::PeriphReset => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let periph: PeriphId = num_traits::FromPrimitive::from_usize(scalar.arg1).unwrap();
                    log::info!("Udma periph reset of {:?}", periph);
                }
            }
            HalOpcode::I2c => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut list = buf.to_original::<I2cTransactions, _>().expect("I2c message format error");
                for transaction in list.transactions.iter_mut() {
                    match transaction.i2c_type {
                        I2cTransactionType::Write => {
                            log::info!(
                                "I2C write of {:x?} to device at {:x}, addr {:x}",
                                transaction.data,
                                transaction.device,
                                transaction.address
                            );
                            transaction.result = I2cResult::Ack(transaction.data.len());
                        }
                        I2cTransactionType::Read | I2cTransactionType::ReadRepeatedStart => {
                            log::info!(
                                "I2C read from device at {:x}, addr {:x}; returning garbage",
                                transaction.device,
                                transaction.address,
                            );
                            transaction.result = I2cResult::Ack(transaction.data.len());
                        }
                    }
                }
                buf.replace(list).expect("I2c message format error");
            }
            HalOpcode::InvalidCall => {
                log::error!("Invalid opcode received: {:?}", msg);
            }
            HalOpcode::Quit => {
                log::info!("Received quit opcode, exiting.");
                break;
            }
        }
    }
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
