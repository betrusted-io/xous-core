use bao1x_api::*;
use bao1x_hal::udma::GlobalConfig;

pub fn start_i2c_service() {
    let _ = std::thread::spawn({
        move || {
            i2c_service();
        }
    });
}

fn i2c_service() -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(bao1x_api::SERVER_NAME_BAO1X_I2C, None).expect("can't register server");

    let iox = IoxHal::new();
    let udma_global = GlobalConfig::new();

    // Note: the I2C handler can be put into a separate thread if we need the main
    // HAL server to not block while a large I2C transaction is being handled. For
    // now this is all placed into a single thread. However, if we ever had a situation
    // where, for example, you had to do a compound I2C transaction and flip a GPIO pin
    // in the middle of that transaction in order for the set of I2C transactions to
    // complete, this implementation would deadlock as it would block on the I2C transaction
    // before handling the GPIO request.
    let i2c_channel = bao1x_hal::board::setup_i2c_pins(&iox);
    udma_global.clock_on(PeriphId::from(i2c_channel));
    let i2c_pages = xous::syscall::map_memory(
        xous::MemoryAddress::new(bao1x_hal::board::I2C_IFRAM_ADDR),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't claim I2C IFRAM page");

    let i2c_ifram = unsafe {
        bao1x_hal::ifram::IframRange::from_raw_parts(
            bao1x_hal::board::I2C_IFRAM_ADDR,
            i2c_pages.as_ptr() as usize,
            i2c_pages.len(),
        )
    };
    let mut i2c = unsafe {
        bao1x_hal::udma::I2cDriver::new_with_ifram(
            i2c_channel,
            400_000,
            bao1x_api::PERCLK,
            i2c_ifram,
            &udma_global,
        )
    };
    let mut msg_opt = None;
    log::debug!("Starting main loop");
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(I2cOpcode::InvalidCall)
        };

        match opcode {
            I2cOpcode::Transaction =>
            // there are no opcode types - this handles exactly one type of message, all others are ignored
            {
                if let Some(msg) = msg_opt.as_mut().unwrap().body.memory_message_mut() {
                    let mut buf = unsafe { xous_ipc::Buffer::from_memory_message_mut(msg) };
                    let mut list = buf.to_original::<I2cTransactions, _>().expect("I2c message format error");
                    for transaction in list.transactions.iter_mut() {
                        match transaction.i2c_type {
                            I2cTransactionType::Write => {
                                match i2c.i2c_write(
                                    transaction.device,
                                    transaction.address,
                                    &transaction.data,
                                ) {
                                    Ok(result) => transaction.result = result,
                                    _ => transaction.result = I2cResult::Nack,
                                }
                            }
                            I2cTransactionType::Read | I2cTransactionType::ReadRepeatedStart => {
                                match i2c.i2c_read(
                                    transaction.device,
                                    transaction.address,
                                    &mut transaction.data,
                                    transaction.i2c_type == I2cTransactionType::ReadRepeatedStart,
                                ) {
                                    Ok(result) => transaction.result = result,
                                    _ => transaction.result = I2cResult::Nack,
                                }
                            }
                        }
                    }
                    buf.replace(list).expect("I2c message format error");
                }
            }
            I2cOpcode::UpdatePerclk => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let new_perclk = scalar.arg1 as u32;
                    i2c.update_perclk(new_perclk);
                }
            }
            I2cOpcode::InvalidCall => {
                log::error!("Unrecognized opcode in I2C: {:?}", opcode);
            }
        }
    }
}
