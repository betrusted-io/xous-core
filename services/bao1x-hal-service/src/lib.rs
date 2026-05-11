pub mod api;
pub mod trng;

use bao1x_api::*;
use bao1x_hal::clocks::ClockOp;
use bao1x_hal::udma::{AdcExtChannel, AdcSource};
use chrono::{DateTime, Offset, TimeZone, Utc};
use num_traits::*;
use xous::{Message, send_message};
use xous_api_susres::api::Opcode as SusresOp;

use crate::api::{TIME_SERVER_PUBLIC, TimeOp};

pub struct UdmaGlobal {
    conn: xous::CID,
}

impl UdmaGlobal {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_BAO1X_HAL).expect("Couldn't connect to bao1x HAL server");
        UdmaGlobal { conn }
    }

    pub fn udma_clock_config(&self, peripheral: PeriphId, enable: bool) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::ConfigureUdmaClock.to_usize().unwrap(),
                peripheral as u32 as usize,
                if enable { 1 } else { 0 },
                0,
                0,
            ),
        )
        .expect("Couldn't setup UDMA clock");
    }

    /// Safety: this event does no checking if an event has been previously mapped. It is up
    /// to the caller to ensure that no events are being stomped on.
    pub unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    ) {
        let et_u32: u32 = event_type.into();
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::ConfigureUdmaEvent.to_usize().unwrap(),
                peripheral as u32 as usize,
                et_u32 as usize,
                to_channel as u32 as usize,
                0,
            ),
        )
        .expect("Couldn't setup UDMA event mapping");
    }

    pub fn reset(&self, peripheral: PeriphId) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::PeriphReset.to_usize().unwrap(),
                peripheral as u32 as usize,
                0,
                0,
                0,
            ),
        )
        .expect("Couldn't setup UDMA clock");
    }
}

pub struct Hal {
    conn: xous::CID,
}
impl Hal {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_BAO1X_HAL).expect("Couldn't connect to bao1x HAL server");
        Hal { conn }
    }

    pub fn set_preemption(&self, on: bool) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::SetPreemptionState.to_usize().unwrap(),
                if on { 1 } else { 0 },
                0,
                0,
                0,
            ),
        )
        .ok();
    }
}

impl UdmaGlobalConfig for UdmaGlobal {
    fn clock(&self, peripheral: PeriphId, enable: bool) { self.udma_clock_config(peripheral, enable); }

    unsafe fn udma_event_map(
        &self,
        peripheral: PeriphId,
        event_type: PeriphEventType,
        to_channel: EventChannel,
    ) {
        self.udma_event_map(peripheral, event_type, to_channel);
    }

    fn reset(&self, peripheral: PeriphId) { self.reset(peripheral); }

    fn irq_status_bits(&self, bank: IrqBank) -> u32 {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                HalOpcode::UdmaIrqStatusBits.to_usize().unwrap(),
                bank as u32 as usize,
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_, value, _, _, _)) => value as u32,
            _ => panic!("Unhandled response on irq_status_bits"),
        }
    }
}

pub struct Rtc {
    conn: xous::CID,
}
impl Rtc {
    pub fn new() -> Self {
        let conn = xous::connect(xous::SID::from_bytes(TIME_SERVER_PUBLIC).unwrap()).unwrap();
        Rtc { conn }
    }

    pub fn set_time<Tz: TimeZone>(&self, datetime: DateTime<Tz>) {
        let utc_time = datetime.with_timezone(&Utc);
        log::info!("Time (UTC): {}", utc_time);
        let since_epoch_ms = utc_time.timestamp_millis();
        xous::send_message(
            self.conn,
            xous::Message::new_scalar(
                TimeOp::SetUtcTimeMs.to_usize().unwrap(),
                ((since_epoch_ms >> 32) & 0xFFFF_FFFF) as usize,
                (since_epoch_ms & 0xFFFF_FFFF) as usize,
                0,
                0,
            ),
        )
        .ok();
        let offset_ms = (datetime.offset().fix().local_minus_utc() as i64) * 1000;
        xous::send_message(
            self.conn,
            xous::Message::new_scalar(
                TimeOp::SetTzOffsetMs.to_usize().unwrap(),
                (((offset_ms as u64) >> 32) & 0xFFFF_FFFF) as usize,
                (offset_ms & 0xFFFF_FFFF) as usize,
                0,
                0,
            ),
        )
        .ok();
    }

    pub fn set_wakeup<Tz: TimeZone>(&self, alarm: DateTime<Tz>) -> Result<usize, xous::Error> {
        let utc_alarm = alarm.with_timezone(&Utc);
        let ts_utc_ms = utc_alarm.timestamp_millis();
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                TimeOp::SetWakeup.to_usize().unwrap(),
                ((ts_utc_ms >> 32) & 0xFFFF_FFFF) as usize,
                (ts_utc_ms & 0xFFFF_FFFF) as usize,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_op, epochs, _, _, _)) => Ok(epochs),
            _ => Err(xous::Error::InternalError),
        }
    }

    pub fn clear_wakeup(&self) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(TimeOp::ClearWakeup.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap();
    }
}
pub struct Adc {
    conn: xous::CID,
}
impl Adc {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_BAO1X_OTHERS).expect("Couldn't connect to bao1x HAL server");
        Adc { conn }
    }

    /// Averaging can range from 1..1024. Out of range values are silently converted to in-range values
    /// based on saturating logic. None for averaging translates to 1.
    ///
    /// For analog channels, the measurement goes from 0..1.208v. There is some nonlinearity observed below
    /// 0.2V, so the "sweet spot" is 0.2V-1.2V.
    pub fn read_raw(&self, channel: AdcSource, averaging: Option<u16>) -> u16 {
        let averaging =
            averaging.unwrap_or(1).min(1).max((bao1x_hal::udma::ADC_RX_BUF_SIZE / size_of::<u32>()) as u16);
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                PeripheralOpcode::ReadAdcChannel.to_usize().unwrap(),
                channel.to_usize(),
                averaging as usize,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar5(_, value, _, _, _)) => value as u16,
            _ => panic!("Unhandled response on Adc::read_raw"),
        }
    }

    /// Safety: this method does no checks for concurrent use of the analog channels.
    /// It is up to the caller to ensure that ADC0-3 (mapped to PA4-7) have no conflicts
    /// with other I/O functions.
    pub unsafe fn enable_channel(&self, channel: AdcExtChannel) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                PeripheralOpcode::EnableChannel.to_usize().unwrap(),
                channel.into(),
                0,
                0,
                0,
            ),
        )
        .ok();
    }
}

pub struct ClockManager {
    conn: xous::CID,
}
impl ClockManager {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns
            .request_connection_blocking(xous_api_susres::api::SERVER_NAME_SUSRES)
            .expect("Couldn't connect to susres server");
        ClockManager { conn }
    }

    pub fn get_fclk(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetFclk.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_aclk(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetAclk.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_hclk(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetHclk.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_iclk(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetIclk.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_pclk(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetPclk.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_per(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetPer.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn get_vco(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::GetVco.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(clk)) => clk as u32,
            _ => unimplemented!(),
        }
    }

    pub fn reset_reason(&self) -> bao1x_api::ResetReason {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::ResetReason.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        ) {
            Ok(xous::Result::Scalar1(reason)) => bao1x_api::ResetReason::new_with_raw_value(reason as u32),
            _ => unimplemented!(),
        }
    }
}
