use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::generated::utra::bio_bdma;

pub struct CanBus {
    bio_ss: Bio,
    clk_pin: u5,
    tx_pin: u5,
    dbg_pin: u5,
    _rx_handle: CoreHandle,
    _tx_handle: CoreHandle,
    rx: CoreCsr,
    tx: CoreCsr,
    resource_grant: ResourceGrant,
}

impl Resources for CanBus {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "CanBus".to_string(),
            cores: vec![
                CoreRequirement::Specific(BioCore::Core0),
                CoreRequirement::Specific(BioCore::Core1),
                CoreRequirement::Specific(BioCore::Core2),
                CoreRequirement::Specific(BioCore::Core3),
            ],
            fifos: vec![Fifo::Fifo0, Fifo::Fifo1, Fifo::Fifo2, Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 2,
        }
    }
}

impl Drop for CanBus {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.clk_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.tx_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.dbg_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl CanBus {
    pub fn new() -> Result<CanBus, BioError> {
        let clk_pin = arbitrary_int::u5::new(17);
        let tx_pin = arbitrary_int::u5::new(16);
        let dbg_pin = arbitrary_int::u5::new(1);
        let frequency = 100_000;

        let mut bio_ss = Bio::new();
        let resource_grant = bio_ss.claim_resources(&CanBus::resource_spec())?;
        let config_rx = CoreConfig { clock_mode: ClockMode::ExternalPin(BioPin::new(clk_pin.as_u8())) };
        let config_tx = CoreConfig { clock_mode: ClockMode::TargetFreqFrac(frequency) };
        let rx_kernel = can_bus_rx_kernel();
        let tx_kernel = can_bus_tx_kernel();

        bio_ss.init_core(resource_grant.cores[2], rx_kernel, config_rx)?;
        bio_ss.init_core(resource_grant.cores[0], tx_kernel, config_tx)?;
        // bio_ss.init_core(resource_grant.cores[3], tx_kernel, config_tx)?;

        bio_ss.claim_dynamic_pin(clk_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(tx_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(dbg_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        let mut io_config = IoConfig::default();
        io_config.mapped = (1 << clk_pin.as_u32()) | (1 << tx_pin.as_u32()) | (1 << dbg_pin.as_u32());
        io_config.i_inv = 1 << clk_pin.as_u32(); // invert clk pin signal
        io_config.mode = IoConfigMode::Overwrite;
        bio_ss.setup_io_config(io_config).unwrap();
        bio_ss.set_core_run_state(&resource_grant, true);
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get Fifo1 handle");
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get Fifo2 handle");
        let mut rx = CoreCsr::from_handle(&rx_handle);
        let mut tx = CoreCsr::from_handle(&tx_handle);

        rx.csr.wo(bio_bdma::SFR_TXF1, 1 << dbg_pin.as_u32()); // dbg pin location
        tx.csr.wo(bio_bdma::SFR_TXF2, 1 << tx_pin.as_u32()); // tx pin location

        Ok(Self {
            bio_ss,
            clk_pin,
            tx_pin,
            dbg_pin,
            _rx_handle: rx_handle,
            _tx_handle: tx_handle,
            rx,
            tx,
            resource_grant,
        })
    }
}

#[rustfmt::skip]
bio_code!(can_bus_rx_kernel, CAN_BUS_RX_START, CAN_BUS_RX_END,

    "mv    x1, x17",         // dbg pin location
    "mv    x26, x1",         // GPIO mask
    "mv    x24, x1",         // set dbg pin as output

"10:",
    "mv x20, x0",
    "mv x22, x1",
    "mv x20, x0",
    "not x23, x1",
    "j 10b"
);

#[rustfmt::skip]
bio_code!(can_bus_tx_kernel, CAN_BUS_TX_START, CAN_BUS_TX_END,

    "mv    x15, x18",        // tx pin location
    "mv    x26, x15",        // GPIO mask
    "mv    x24, x15",        // set tx pin as output

"10:",
    "mv x20, x0",
    "mv x22, x15",
    "mv x20, x0",
    "not x23, x15",
    "j 10b"
);
