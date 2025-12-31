use std::marker::PhantomData;

use bao1x_api::bio::*;
use num_traits::*;
use rkyv::option::ArchivedOption;
use utralib::*;
use xous::{CID, MemoryAddress, Message, send_message};
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

#[cfg(feature = "std")]
impl<'a> BioApi<'a> for Bio {
    fn init_core(
        &mut self,
        core: BioCore,
        code: &[u8],
        offset: usize,
        config: CoreConfig,
    ) -> Result<Option<u32>, BioError> {
        let mut config = CoreInitRkyv {
            core,
            offset,
            actual_freq: None,
            config,
            code: [0u8; 4096],
            result: BioError::Uninit,
        };
        config.code[..code.len()].copy_from_slice(code);
        // this should automatically allocate 2 pages because the sizeof() type of CoreInitRkyv is over 4096
        let mut buf = Buffer::into_buf(config).unwrap();
        buf.lend_mut(self.conn, BioOp::InitCore.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        let returned_config = buf.as_flat::<CoreInitRkyv, _>().unwrap();

        match returned_config.result {
            ArchivedBioError::None => {
                if let ArchivedOption::Some(freq) = returned_config.actual_freq {
                    Ok(Some(freq.to_native()))
                } else {
                    Ok(None)
                }
            }
            ArchivedBioError::Uninit => panic!("Error in message passing"),
            ArchivedBioError::InvalidCore => Err(BioError::InvalidCore),
            ArchivedBioError::NoFreeMachines => Err(BioError::NoFreeMachines),
            ArchivedBioError::ResourceInUse => Err(BioError::ResourceInUse),
            ArchivedBioError::Oom => Err(BioError::Oom),
            _ => Err(BioError::InternalError),
        }
    }

    fn de_init_core(&mut self, core: BioCore) -> Result<(), BioError> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::DeInitCore.to_usize().unwrap(), core as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    unsafe fn get_core_handle(&'a self, fifo: Fifo) -> Result<Option<CoreHandle<'a>>, BioError> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetCoreHandle.to_usize().unwrap(), fifo as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))?
        {
            xous::Result::Scalar5(_, valid, _, _, _) => {
                if valid == 0 {
                    // no handles available
                    return Ok(None);
                }
                let base = match fifo {
                    Fifo::Fifo0 => utralib::HW_BIO_FIFO0_BASE,
                    Fifo::Fifo1 => utralib::HW_BIO_FIFO1_BASE,
                    Fifo::Fifo2 => utralib::HW_BIO_FIFO2_BASE,
                    Fifo::Fifo3 => utralib::HW_BIO_FIFO3_BASE,
                };
                // the actual mapping is requested in the process space of the caller: this allows
                // us to directly interact with the BIO without having to use a syscall.
                if let Ok(virtual_page) = xous::map_memory(
                    MemoryAddress::new(base),
                    None,
                    utralib::HW_BIO_FIFO0_MEM_LEN,
                    xous::MemoryFlags::R | xous::MemoryFlags::W,
                ) {
                    Ok(Some(CoreHandle::new(
                        self.conn,
                        virtual_page.as_ptr() as usize,
                        arbitrary_int::u2::from_u32(fifo as u32),
                    )))
                } else {
                    Err(BioError::Oom)
                }
            }
            _ => unimplemented!("Unhandled return type on get_core_handle()"),
        }
    }

    fn update_bio_freq(&mut self, freq: u32) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::UpdateBioFreq.to_usize().unwrap(), freq as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))
        .unwrap()
        {
            xous::Result::Scalar5(_, freq, _, _, _) => freq as u32,
            _ => unimplemented!("Unhandled return type"),
        }
    }

    fn get_bio_freq(&self) -> u32 {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetBioFreq.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))
        .unwrap()
        {
            xous::Result::Scalar5(_, freq, _, _, _) => freq as u32,
            _ => unimplemented!("Unhandled return type"),
        }
    }

    fn get_core_freq(&self, core: BioCore) -> Option<u32> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(BioOp::GetCoreFreq.to_usize().unwrap(), core as usize, 0, 0, 0),
        )
        .map_err(|e| <xous::Error as Into<BioError>>::into(e))
        .unwrap()
        {
            xous::Result::Scalar5(_, freq, valid, _, _) => {
                if valid != 0 {
                    Some(freq as u32)
                } else {
                    None
                }
            }
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

    fn set_core_state(&mut self, which: [CoreRunSetting; 4]) -> Result<(), BioError> {
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

    fn setup_dma_windows(&mut self, windows: DmaFilterWindows) -> Result<(), BioError> {
        let buf = Buffer::into_buf(windows).unwrap();
        buf.lend(self.conn, BioOp::DmaWindows.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_fifo_event_triggers(&mut self, config: FifoEventConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::FifoEventTriggers.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_io_config(&mut self, config: IoConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::IoConfig.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }

    fn setup_irq_config(&mut self, config: IrqConfig) -> Result<(), BioError> {
        let buf = Buffer::into_buf(config).unwrap();
        buf.lend(self.conn, BioOp::IrqConfig.to_u32().unwrap())
            .map_err(|e| <xous::Error as Into<BioError>>::into(e))?;
        Ok(())
    }
}
