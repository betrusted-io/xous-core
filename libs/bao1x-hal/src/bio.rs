use std::marker::PhantomData;

use bao1x_api::bio::*;
use num_traits::*;
use utralib::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;

pub struct Bio {
    conn: CID,
}

impl Bio {
    pub fn new() -> Self {
        let xns = xous_api_names::XousNames::new().unwrap();
        let conn = xns.request_connection_blocking(BIO_SERVER_NAME).expect("couldn't connect to BIO server");
        Self { conn }
    }
}

pub struct CoreCsr<'a> {
    pub csr: CSR<u32>,
    _lifetime: PhantomData<&'a ()>,
}
impl<'a> CoreCsr<'a> {
    pub fn from_handle(handle: CoreHandle<'a>) -> Self {
        let (pointer, lifetime) = unsafe { handle.handle() };
        // safety: this structure tracks the lifetime of the handle, and is therefore safe.
        Self { csr: CSR::new(pointer as *mut u32), _lifetime: lifetime }
    }
}

impl<'a> BioApi<'a> for Bio {
    fn init_core(
        &self,
        core: BioCore,
        code: &[u8],
        offset: usize,
        config: CoreConfig,
    ) -> Result<(), BioError> {
        let mut config = CoreInitRkyv { core, offset, config: config.into(), code: [0u8; 4096] };
        config.code[..code.len()].copy_from_slice(code);
        // this should automatically allocate 2 pages because the sizeof() type of CoreInitRkyv is over 4096
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::InitCore.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn de_init_core(&self, core: BioCore) -> Result<(), BioError> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::DeInitCore.to_usize().unwrap(), core as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    unsafe fn get_core_handle(&'a self) -> Result<CoreHandle<'a>, BioError> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetCoreHandle.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?
        {
            xous::Result::Scalar5(_, handle, _, _, _) => Ok(CoreHandle::new(self.conn, handle)),
            _ => unimplemented!("Unhandled return type on get_core_handle()"),
        }
    }

    fn get_freq(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetCoreFreq.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))
        .unwrap()
        {
            xous::Result::Scalar5(_, freq, _, _, _) => freq as u32,
            _ => unimplemented!("Unhandled return type"),
        }
    }

    fn get_version(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetVersion.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))
        .unwrap()
        {
            xous::Result::Scalar5(_, version, _, _, _) => version as u32,
            _ => unimplemented!("Unhandled return type"),
        }
    }

    fn set_core_freq(&self, freq: u32) -> Result<u32, BioError> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::SetCoreFreq.to_usize().unwrap(), freq as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?
        {
            xous::Result::Scalar5(_, previous_freq, _, _, _) => Ok(previous_freq as u32),
            _ => unimplemented!("Unhandled return type"),
        }
    }

    fn set_core_state(&self, which: [CoreRunSetting; 4]) -> Result<(), BioError> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                BioOp::CoreState.to_usize().unwrap(),
                which[0] as usize,
                which[1] as usize,
                which[2] as usize,
                which[3] as usize,
            ),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_dma_windows(&self, windows: DmaFilterWindows) -> Result<(), BioError> {
        let buf = Buffer::into_buf(windows).unwrap();
        buf.lend(self.conn, BioOp::DmaWindows.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_fifo_event_triggers(&self, config: FifoEventConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::FifoEventTriggers.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_io_config(&self, config: IoConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::IoConfig.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_irq_config(&self, config: IrqConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::IrqConfig.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }
}
