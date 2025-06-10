use cramium_api::{I2cApi, I2cResult};

pub const BMP180_ADDR: u8 = 0x77;
#[cfg(feature = "std")]
const REG_CALIB_START: u8 = 0xAA;
const REG_CTRL: u8 = 0xF4;
const REG_DATA_START: u8 = 0xF6;
const CMD_READ_TEMP: u8 = 0x2E;

#[derive(Debug, Clone, Copy)]

#[allow(dead_code)]
struct Bmp180Calibration {
    ac1: i16,
    ac2: i16,
    ac3: i16,
    ac4: u16,
    ac5: u16,
    ac6: u16,
    b1: i16,
    b2: i16,
    mb: i16,
    mc: i16,
    md: i16,
}

pub struct Bmp180 {
    calibration: Bmp180Calibration,
}

impl Bmp180 {
    #[cfg(feature = "std")]
    pub fn new(i2c: &mut dyn I2cApi) -> Result<Self, I2cResult> {
        let mut cal_buf = [0u8; 22];

        match i2c.i2c_read(BMP180_ADDR, REG_CALIB_START, &mut cal_buf, true) {
            Ok(i2c_result) => match i2c_result {
                I2cResult::Ack(_) => (),
                other => return Err(other),
            },
            Err(_) => return Err(I2cResult::InternalError),
        }

        // note: calibration data is Big Endian, hence the from_be_bytes
        let calibration = Bmp180Calibration {
            ac1: i16::from_be_bytes([cal_buf[0], cal_buf[1]]),
            ac2: i16::from_be_bytes([cal_buf[2], cal_buf[3]]),
            ac3: i16::from_be_bytes([cal_buf[4], cal_buf[5]]),
            ac4: u16::from_be_bytes([cal_buf[6], cal_buf[7]]),
            ac5: u16::from_be_bytes([cal_buf[8], cal_buf[9]]),
            ac6: u16::from_be_bytes([cal_buf[10], cal_buf[11]]),
            b1: i16::from_be_bytes([cal_buf[12], cal_buf[13]]),
            b2: i16::from_be_bytes([cal_buf[14], cal_buf[15]]),
            mb: i16::from_be_bytes([cal_buf[16], cal_buf[17]]),
            mc: i16::from_be_bytes([cal_buf[18], cal_buf[19]]),
            md: i16::from_be_bytes([cal_buf[20], cal_buf[21]]),
        };

        if calibration.ac1 == 0
            || calibration.ac2 == 0
            || calibration.ac3 == 0
            || calibration.ac4 == 0
            || calibration.ac5 == 0
            || calibration.ac6 == 0
            || calibration.b1 == 0
            || calibration.b2 == 0
            || calibration.mb == 0
            || calibration.mc == 0
            || calibration.md == 0
            || calibration.ac1 == -1
        {
            return Err(I2cResult::InternalError);
        }

        Ok(Bmp180 { calibration })
    }

    pub fn read_temperature(&self, i2c: &mut dyn I2cApi) -> Result<f32, I2cResult> {
        match i2c.i2c_write(BMP180_ADDR, REG_CTRL, &[CMD_READ_TEMP]) {
            Ok(I2cResult::Ack(_)) => (),
            Ok(other) => return Err(other),
            Err(_) => return Err(I2cResult::InternalError),
        }

        self.delay(5);

        let mut temp_buffer = [0u8; 2];
        match i2c.i2c_read(BMP180_ADDR, REG_DATA_START, &mut temp_buffer, true) {
            Ok(I2cResult::Ack(_)) => (),
            Ok(other) => return Err(other),
            Err(_) => return Err(I2cResult::InternalError),
        }

        let ut = i16::from_be_bytes(temp_buffer) as i32;

        let cal = &self.calibration;
        let x1 = (ut - cal.ac6 as i32) * cal.ac5 as i32 >> 15;
        let x2 = (cal.mc as i32 * 2048) / (x1 + cal.md as i32);
        let b5 = x1 + x2;
        let temp = ((b5 + 8) >> 4) as f32 / 10.0;

        Ok(temp)
    }

    pub fn delay(&self, quantum: usize) {
        #[cfg(feature = "std")]
        {
            let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
            tt.sleep_ms(quantum).ok();
        }
        #[cfg(not(feature = "std"))]
        {
            use utralib::{CSR, utra};
            // abuse the d11ctime timer to create some time-out like thing
            let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
            d11c.wfo(utra::d11ctime::CONTROL_COUNT, 333_333); // 1.0ms per interval
            let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
            for _ in 0..quantum {
                while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
                polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
            }
            // we have to split this because we don't know where we caught the previous interval
            if quantum == 1 {
                while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
            }
        }
    }
}
