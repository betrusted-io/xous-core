use cramium_api::*;

pub const AXP2101_DEV: u8 = 0x34;

const REG_DCDC_ENA: usize = 0x80;
const REG_DCDC_PWM: usize = 0x81;
const REG_DCDC1_V: usize = 0x82;
const REG_DCDC2_V: usize = 0x83;
const REG_DCDC3_V: usize = 0x84;
const REG_DCDC4_V: usize = 0x85;
#[allow(dead_code)]
const REG_DCDC5_V: usize = 0x86;
const REG_LDO1_ENA: usize = 0x90;
const REG_LDO2_ENA: usize = 0x91;
#[allow(dead_code)]
const REG_ALDO1_V: usize = 0x92;
#[allow(dead_code)]
const REG_ALDO2_V: usize = 0x93;
#[allow(dead_code)]
const REG_ALDO3_V: usize = 0x94;
#[allow(dead_code)]
const REG_ALDO4_V: usize = 0x95;
#[allow(dead_code)]
const REG_BLDO1_V: usize = 0x96;
#[allow(dead_code)]
const REG_BLDO2_V: usize = 0x97;
#[allow(dead_code)]
const REG_CPUSLDO_V: usize = 0x98;
#[allow(dead_code)]
const REG_DLDO1_V: usize = 0x99;
#[allow(dead_code)]
const REG_DLDO2_V: usize = 0x9A;

const REG_IRQ_ENABLE0: u8 = 0x40;
#[allow(dead_code)]
const REG_IRQ_ENABLE1: u8 = 0x41;
#[allow(dead_code)]
const REG_IRQ_ENABLE2: u8 = 0x42;
const REG_IRQ_STATUS0: u8 = 0x48;
const REG_IRQ_STATUS1: u8 = 0x49;
#[allow(dead_code)]
const REG_IRQ_STATUS2: u8 = 0x4A;
const VBUS_INSERT_MASK: u8 = 0x80;
const VBUS_REMOVE_MASK: u8 = 0x40;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum VbusIrq {
    None,
    Insert,
    Remove,
    InsertAndRemove,
}

/// From the raw u8 read back from the register
impl From<u8> for VbusIrq {
    fn from(value: u8) -> Self {
        if (value & VBUS_INSERT_MASK) == 0 && (value & VBUS_REMOVE_MASK) == 0 {
            VbusIrq::None
        } else if (value & VBUS_INSERT_MASK) != 0 && (value & VBUS_REMOVE_MASK) == 0 {
            VbusIrq::Insert
        } else if (value & VBUS_INSERT_MASK) == 0 && (value & VBUS_REMOVE_MASK) != 0 {
            VbusIrq::Remove
        } else {
            VbusIrq::InsertAndRemove
        }
    }
}

#[repr(u8)]
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum WhichLdo {
    Aldo1 = 0,
    Aldo2 = 1,
    Aldo3 = 2,
    Aldo4 = 3,
    Bldo1 = 4,
    Bldo2 = 5,
    Cpusldo1 = 6,
    Dldo1 = 7,
    Dldo2 = 8,
}
impl Into<f32> for WhichLdo {
    fn into(self) -> f32 {
        match self {
            Self::Cpusldo1 | Self::Dldo1 => 0.050,
            _ => 0.100,
        }
    }
}
impl From<usize> for WhichLdo {
    fn from(value: usize) -> Self {
        match value {
            0 => Self::Aldo1,
            1 => Self::Aldo2,
            2 => Self::Aldo3,
            3 => Self::Aldo4,
            4 => Self::Bldo1,
            5 => Self::Bldo2,
            6 => Self::Cpusldo1,
            7 => Self::Dldo1,
            8 => Self::Dldo2,
            _ => panic!("bad WhichLdo"),
        }
    }
}

#[repr(usize)]
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum WhichDcDc {
    Dcdc1 = 0,
    Dcdc2 = 1,
    Dcdc3 = 2,
    Dcdc4 = 3,
    Dcdc5 = 4,
}
impl From<usize> for WhichDcDc {
    fn from(value: usize) -> Self {
        match value {
            0 => Self::Dcdc1,
            1 => Self::Dcdc2,
            2 => Self::Dcdc3,
            3 => Self::Dcdc4,
            4 => Self::Dcdc5,
            _ => panic!("bad WhichDcDc"),
        }
    }
}

// Deriving this causes floating point converters to be included in the output
// which is +40k of code
// #[derive(Debug)]
pub struct Axp2101 {
    pub dcdc_ena: [bool; 5],
    pub fast_ramp: bool,
    pub force_ccm: bool,
    pub dcdc_v_dvm: [(f32, bool); 5],
    pub ldo_ena: [bool; 9],
    pub ldo_v: [f32; 9],
}

impl Axp2101 {
    pub fn new(i2c: &mut dyn I2cApi) -> Result<Axp2101, xous::Error> {
        let mut s = Axp2101 {
            dcdc_ena: [false; 5],
            fast_ramp: false,
            force_ccm: false,
            dcdc_v_dvm: [(0.0, false); 5],
            ldo_ena: [false; 9],
            ldo_v: [0.0; 9],
        };
        s.update(i2c)?;
        Ok(s)
    }

    pub fn update(&mut self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        let mut buf = [0u8; 0xb0];
        i2c.i2c_read(AXP2101_DEV, 0x0, &mut buf, false)?;

        (self.dcdc_ena, self.fast_ramp, self.force_ccm) = parse_dcdc_ena(buf[REG_DCDC_ENA]);
        self.dcdc_v_dvm = [
            parse_dcdc(buf[REG_DCDC1_V], WhichDcDc::Dcdc1),
            parse_dcdc(buf[REG_DCDC2_V], WhichDcDc::Dcdc2),
            parse_dcdc(buf[REG_DCDC3_V], WhichDcDc::Dcdc3),
            parse_dcdc(buf[REG_DCDC4_V], WhichDcDc::Dcdc4),
            parse_dcdc(buf[REG_DCDC5_V], WhichDcDc::Dcdc5),
        ];
        for (i, ena) in self.ldo_ena.iter_mut().enumerate() {
            if i < 8 {
                *ena = (buf[REG_LDO1_ENA] >> i) & 1 != 0;
            } else {
                *ena = (buf[REG_LDO2_ENA] & 1) != 0;
            }
        }
        for (i, v) in self.ldo_v.iter_mut().enumerate() {
            *v = parse_ldo(buf[REG_ALDO1_V + i], WhichLdo::from(i));
        }
        Ok(())
    }

    pub fn set_dcdc(
        &mut self,
        i2c: &mut dyn I2cApi,
        setting: Option<(f32, bool)>,
        which: WhichDcDc,
    ) -> Result<(), xous::Error> {
        if let Some((voltage, dvm)) = setting {
            match encode_dcdc(voltage, dvm, which) {
                Some(code) => {
                    self.dcdc_v_dvm[which as usize] = (voltage, dvm);
                    self.dcdc_ena[which as usize] = true;
                    i2c.i2c_write(AXP2101_DEV, (REG_DCDC1_V + which as usize) as u8, &[code]).map(|_| ())?;
                    i2c.i2c_write(AXP2101_DEV, REG_DCDC_ENA as u8, &[self.encode_dcdc_ena()]).map(|_| ())
                }
                None => Err(xous::Error::InvalidLimit),
            }
        } else {
            self.dcdc_ena[which as usize] = false;
            i2c.i2c_write(AXP2101_DEV, REG_DCDC_ENA as u8, &[self.encode_dcdc_ena()]).map(|_| ())
        }
    }

    pub fn set_ldo(
        &mut self,
        i2c: &mut dyn I2cApi,
        setting: Option<f32>,
        which: WhichLdo,
    ) -> Result<(), xous::Error> {
        if let Some(voltage) = setting {
            match encode_ldo(voltage, which) {
                Some(code) => {
                    self.ldo_v[which as usize] = voltage;
                    self.ldo_ena[which as usize] = true;
                    let ctl = self.encode_ldo_ena();
                    i2c.i2c_write(AXP2101_DEV, (REG_ALDO1_V + which as usize) as u8, &[code]).map(|_| ())?;
                    i2c.i2c_write(AXP2101_DEV, REG_LDO1_ENA as u8, &ctl).map(|_| ())
                }
                None => Err(xous::Error::InvalidLimit),
            }
        } else {
            self.ldo_ena[which as usize] = false;
            let ctl = self.encode_ldo_ena();
            i2c.i2c_write(AXP2101_DEV, REG_LDO1_ENA as u8, &ctl).map(|_| ())
        }
    }

    fn encode_dcdc_ena(&self) -> u8 {
        let mut code = 0;
        for (i, &ena) in self.dcdc_ena.iter().enumerate() {
            if ena {
                code |= 1 << i;
            }
        }
        if self.fast_ramp {
            code |= 1 << 5;
        }
        if self.force_ccm {
            code |= 1 << 6;
        }
        code
    }

    fn encode_ldo_ena(&self) -> [u8; 2] {
        let mut ctl = [0u8; 2];
        for (i, &ena) in self.ldo_ena.iter().enumerate() {
            if i < 8 {
                if ena {
                    ctl[0] |= 1 << i;
                }
            } else {
                if ena {
                    ctl[1] = 1;
                }
            }
        }
        ctl
    }

    /// This will clear all other IRQ sources except VBUS IRQ
    /// If we need to take more IRQ sources then this API will need to be refactored.
    pub fn setup_vbus_irq(&mut self, i2c: &mut dyn I2cApi, mode: VbusIrq) -> Result<(), xous::Error> {
        let data = match mode {
            VbusIrq::None => 0u8,
            VbusIrq::Insert => VBUS_INSERT_MASK,
            VbusIrq::Remove => VBUS_REMOVE_MASK,
            VbusIrq::InsertAndRemove => VBUS_INSERT_MASK | VBUS_REMOVE_MASK,
        };
        // ENABLE1 has the code we want to target, but the rest also needs to be cleared so
        // fill the values in with 0.
        i2c.i2c_write(AXP2101_DEV, REG_IRQ_ENABLE0, &[0, data, 0]).map(|_| ())?;

        // clear the status bits
        let mut status = [0u8; 3];
        i2c.i2c_read(AXP2101_DEV, REG_IRQ_STATUS0, &mut status, false)?;
        i2c.i2c_write(AXP2101_DEV, REG_IRQ_STATUS0, &status).map(|_| ())
    }

    pub fn get_vbus_irq_status(&self, i2c: &mut dyn I2cApi) -> Result<VbusIrq, xous::Error> {
        let mut buf = [0u8];
        i2c.i2c_read(AXP2101_DEV, REG_IRQ_STATUS1, &mut buf, false)?;
        Ok(VbusIrq::from(buf[0]))
    }

    /// This will clear all pending IRQs, regardless of the setup
    pub fn clear_vbus_irq_pending(&mut self, i2c: &mut dyn I2cApi) -> Result<(), xous::Error> {
        let data = VBUS_INSERT_MASK | VBUS_REMOVE_MASK;
        i2c.i2c_write(AXP2101_DEV, REG_IRQ_STATUS1, &[data]).map(|_| ())
    }

    pub fn set_pwm_mode(
        &mut self,
        i2c: &mut dyn I2cApi,
        which: WhichDcDc,
        always: bool,
    ) -> Result<(), xous::Error> {
        match which {
            WhichDcDc::Dcdc5 => Err(xous::Error::BadAddress),
            _ => {
                let mut buf = [0u8];
                i2c.i2c_read(AXP2101_DEV, REG_DCDC_PWM as u8, &mut buf, false).map(|_| ())?;
                if always {
                    buf[0] |= 4u8 << (which as usize as u8);
                } else {
                    buf[0] &= !(4u8 << (which as usize as u8));
                }
                i2c.i2c_write(AXP2101_DEV, REG_DCDC_PWM as u8, &buf).map(|_| ())
            }
        }
    }

    pub fn debug(&mut self, i2c: &mut dyn I2cApi) {
        let mut buf = [0u8, 0u8];
        i2c.i2c_read(AXP2101_DEV, REG_DCDC_ENA as u8, &mut buf, false).unwrap();
        crate::println!("ena|pwm bef: {:x?}", buf);
        // force CCM mode
        i2c.i2c_write(AXP2101_DEV, REG_DCDC_ENA as u8, &[buf[0] | 0b0100_0000]).unwrap();
        // disable spreading, force PWM on DCDC2
        i2c.i2c_write(AXP2101_DEV, REG_DCDC_PWM as u8, &[(buf[1] & 0b0011_1111) | 0b0000_1000]).unwrap();
        i2c.i2c_read(AXP2101_DEV, REG_DCDC_ENA as u8, &mut buf, false).unwrap();
        crate::println!("ena|pwm aft: {:x?}", buf);
    }
}

pub fn parse_dcdc_ena(d: u8) -> ([bool; 5], bool, bool) {
    let mut enable = [false; 5];
    let mut fast_ramp = false;
    let mut force_ccm = false;

    for (i, ena) in enable.iter_mut().enumerate() {
        if ((d >> i) & 1) == 1 {
            *ena = true;
        }
    }
    if ((d >> 5) & 1) == 1 {
        fast_ramp = true;
    }
    if ((d >> 6) & 1) == 1 {
        force_ccm = true;
    }
    (enable, fast_ramp, force_ccm)
}

pub fn encode_dcdc(v: f32, dvm: bool, which: WhichDcDc) -> Option<u8> {
    match which {
        WhichDcDc::Dcdc1 => {
            if v < 1.5 || v > 3.4 {
                None
            } else {
                let code = (v - 1.5) / 0.100;
                return Some(code as u8 | if dvm { 0x80 } else { 0x0 });
            }
        }
        _ => {
            if which == WhichDcDc::Dcdc2 || which == WhichDcDc::Dcdc3 {
                if v < 0.5 || v > 1.54 {
                    return None;
                }
            } else if which == WhichDcDc::Dcdc4 {
                if v < 0.5 || v > 1.84 {
                    return None;
                }
            } else {
                // must be Dcdc5
                if v < 1.4 || v > 3.7 {
                    return None;
                } else {
                    let code = (v - 1.4) / 0.100;
                    return Some(code as u8);
                }
            }
            if v < 1.22 {
                let code = (v - 0.5) / 0.010;
                Some(code as u8)
            } else {
                // high side already bounds checked above
                let code = (v - 1.22) / 0.020;
                Some(code as u8 + 70)
            }
        }
    }
}

pub fn parse_dcdc(d: u8, which: WhichDcDc) -> (f32, bool) {
    match which {
        WhichDcDc::Dcdc1 => {
            let step = 0.10f32;
            let dvm = (d & 0x80) != 0;
            let voltage = ((d & 0x3F) as f32) * step + 1.5;
            (voltage, dvm)
        }
        _ => {
            let step = if (d & 0x7F) <= 71 { 0.010f32 } else { 0.020f32 };
            let dvm = (d & 0x80) != 0;
            let voltage = ((d & 0x7F) as f32) * step + 0.5;
            (voltage, dvm)
        }
    }
}

pub fn parse_ldo(code: u8, ldo: WhichLdo) -> f32 {
    let step: f32 = ldo.into();
    0.5f32 + (code as f32) * step
}

// returns a tuple of (code, register)
pub fn encode_ldo(v: f32, ldo: WhichLdo) -> Option<u8> {
    let step: f32 = ldo.into();
    if step == 0.10 {
        if v < 0.5 || v > 3.5 {
            return None;
        }
    } else if step == 0.050 {
        if v < 0.5 || v > 1.4 {
            return None;
        }
    }
    Some(((v - 0.5) / step) as u8)
}
