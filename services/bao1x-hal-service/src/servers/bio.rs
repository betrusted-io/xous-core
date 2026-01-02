use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio_hw;

pub fn start_bio_service(clk_freq: u32) {
    std::thread::spawn(move || {
        bio_service(clk_freq);
    });
}

fn bio_service(clk_freq: u32) {
    let xns = xous_names::XousNames::new().unwrap();
    // claim the server name
    let sid = xns.register_name(BIO_SERVER_NAME, None).unwrap();

    let mut resource_tracker = ResourceTracker::new();

    let mut bio_ss = bio_hw::BioSharedState::new(clk_freq);
    // on baosec platforms, the TRNG occupies core0 and FIFO0. Mark these resources as used.
    // The setup of the TRNG BIO application happened in the bootloader, so we just need to
    // mark the resources as taken here.
    #[cfg(feature = "board-baosec")]
    {
        bio_ss.handle_used = [true, false, false, false];
        bio_ss.core_config =
            [Some(CoreConfig { clock_mode: ClockMode::FixedDivider(1, 0) }), None, None, None];
        resource_tracker.reserve_boot_resources("TRNG", Some(BioCore::Core0), Some(Fifo::Fifo0));
    }
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(BioOp::InvalidCall)
        };
        log::debug!("{:?}", opcode);
        match opcode {
            BioOp::InitCore => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut config = buf.to_original::<CoreInitRkyv, _>().unwrap();
                match bio_ss.init_core(config.core, &config.code, config.offset, config.config) {
                    Ok(freq) => {
                        config.actual_freq = freq;
                        config.result = BioError::None;
                    }
                    Err(e) => {
                        config.result = e;
                        config.actual_freq = None;
                    }
                }
                buf.replace(config).unwrap();
            }

            BioOp::DeInitCore => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let core = scalar.arg1;
                    bio_ss.de_init_core(core.into()).unwrap();
                }
            }

            BioOp::GetCoreHandle => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let index = scalar.arg1;
                    if !bio_ss.handle_used[index] {
                        bio_ss.handle_used[index] = true;
                        scalar.arg1 = 1; // set valid bit
                    } else {
                        scalar.arg1 = 0; // set invalid - handle already in use
                    }
                }
            }

            BioOp::ReleaseCoreHandle => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    // caller should have *already* de-allocated the handle on their side to avoid a race
                    // condition
                    let core = scalar.arg1;
                    log::debug!("core handle {} released", core);
                    bio_ss.handle_used[core] = false;
                    // that's it - all the bookkeeping is now done.
                }
            }

            BioOp::UpdateBioFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let new_freq = scalar.arg1;
                    // returns the old freq
                    scalar.arg1 = bio_ss.update_bio_freq(new_freq as u32) as usize;
                }
            }

            BioOp::GetBioFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    scalar.arg1 = bio_ss.get_bio_freq() as usize;
                }
            }

            BioOp::GetCoreFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let bio_core: BioCore = scalar.arg1.into();
                    let result = bio_ss.get_core_freq(bio_core);
                    if let Some(freq) = result {
                        scalar.arg1 = freq as usize;
                        scalar.arg2 = 1;
                    } else {
                        scalar.arg2 = 0;
                    }
                }
            }

            BioOp::GetVersion => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    scalar.arg1 = bio_ss.get_version() as usize;
                }
            }

            BioOp::CoreState => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let which =
                        [scalar.arg1.into(), scalar.arg2.into(), scalar.arg3.into(), scalar.arg4.into()];
                    log::debug!("setting: {:?}", which);
                    bio_ss.set_core_state(which).unwrap();
                    log::debug!("core state: {:x}", bio_ss.bio.r(utralib::utra::bio_bdma::SFR_CTRL));
                }
            }

            BioOp::DmaWindows => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let windows = buf.to_original::<DmaFilterWindows, _>().unwrap();
                bio_ss.setup_dma_windows(windows).unwrap();
            }

            BioOp::FifoEventTriggers => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<FifoEventConfig, _>().unwrap();
                bio_ss.setup_fifo_event_triggers(config).unwrap();
            }

            BioOp::IoConfig => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<IoConfig, _>().unwrap();
                bio_ss.setup_io_config(config).unwrap();
            }

            BioOp::IrqConfig => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<IrqConfig, _>().unwrap();
                bio_ss.setup_irq_config(config).unwrap();
            }

            BioOp::ClaimResources => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut request = buf.to_original::<ClaimResourcesRequest, _>().unwrap();
                match resource_tracker.claim(&request.spec) {
                    Ok(grant) => {
                        request.grant = Some(grant);
                        request.error = ResourceError::None;
                    }
                    Err(e) => {
                        request.grant = None;
                        request.error = e;
                    }
                }
                buf.replace(request).unwrap();
            }

            BioOp::ReleaseResources => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let grant_id = scalar.arg1 as u32;
                    match resource_tracker.release(grant_id) {
                        Ok(()) => scalar.arg1 = 0,
                        Err(_) => scalar.arg1 = 1,
                    }
                }
            }

            BioOp::ResourceAvailability => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut response = buf.to_original::<ResourceAvailabilityResponse, _>().unwrap();
                response.availability = resource_tracker.get_availability();
                buf.replace(response).unwrap();
            }

            BioOp::CheckResources => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut request = buf.to_original::<ClaimResourcesRequest, _>().unwrap();
                request.error = match resource_tracker.check_spec(&request.spec) {
                    Ok(()) => ResourceError::None,
                    Err(e) => e,
                };
                buf.replace(request).unwrap();
            }

            BioOp::CheckResourcesBatch => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut request = buf.to_original::<CheckResourcesBatchRequest, _>().unwrap();
                request.error = match resource_tracker.check_specs_batch(&request.specs) {
                    Ok(()) => ResourceError::None,
                    Err(e) => e,
                };
                buf.replace(request).unwrap();
            }

            BioOp::ClaimDynamicPin => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut request = buf.to_original::<DynamicPinRequest, _>().unwrap();
                request.error = match resource_tracker.claim_dynamic_pin(request.pin, &request.claimer) {
                    Ok(()) => ResourceError::None,
                    Err(e) => e,
                };
                buf.replace(request).unwrap();
            }

            BioOp::ReleaseDynamicPin => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut request = buf.to_original::<DynamicPinRequest, _>().unwrap();
                request.error = match resource_tracker.release_dynamic_pin(request.pin, &request.claimer) {
                    Ok(()) => ResourceError::None,
                    Err(e) => e,
                };
                buf.replace(request).unwrap();
            }

            BioOp::InvalidCall => panic!("Invalid BioOp"),
        }
    }
}

/// Server-side resource tracking state
pub struct ResourceTracker {
    /// For each core: None if available, Some(claimer_name) if claimed
    cores: [Option<String>; 4],
    /// For each FIFO: None if available, Some(claimer_name) if claimed
    fifos: [Option<String>; 4],
    /// For each pin: None if available, Some(claimer_name) if claimed
    pins: [Option<String>; 32],
    /// Dynamic pin reservations per claimer
    dynamic_reservations: Vec<(String, u8)>,
    /// Next grant ID to issue
    next_grant_id: u32,
    /// Active grants (grant_id -> claimer name)
    active_grants: Vec<(u32, String)>,
}

impl ResourceTracker {
    pub fn new() -> Self {
        Self {
            cores: Default::default(),
            fifos: Default::default(),
            pins: Default::default(),
            dynamic_reservations: Vec::new(),
            next_grant_id: 1,
            active_grants: Vec::new(),
        }
    }

    /// Pre-claim specific resources for boot-time allocations (e.g., TRNG on baosec)
    #[allow(dead_code)]
    pub fn reserve_boot_resources(&mut self, claimer: &str, core: Option<BioCore>, fifo: Option<Fifo>) {
        if let Some(c) = core {
            self.cores[c as usize] = Some(claimer.to_string());
        }
        if let Some(f) = fifo {
            self.fifos[f as usize] = Some(claimer.to_string());
        }
    }

    pub fn get_availability(&self) -> ResourceAvailability {
        ResourceAvailability {
            cores: self.cores.clone(),
            fifos: self.fifos.clone(),
            pins: self.pins.clone(),
            dynamic_pins_reserved: self.dynamic_reservations.iter().map(|(_, n)| n).sum(),
        }
    }

    /// Check if a spec can be satisfied against current state.
    /// Returns Ok(()) if possible, Err with details if not.
    pub fn check_spec(&self, spec: &ResourceSpec) -> Result<(), ResourceError> {
        // Check specific cores
        for req in &spec.cores {
            if let CoreRequirement::Specific(core) = req {
                if let Some(claimer) = &self.cores[*core as usize] {
                    return Err(ResourceError::CoreUnavailable { core: *core, claimed_by: claimer.clone() });
                }
            }
        }

        // Count "Any" core requests and available cores
        let any_count = spec.cores.iter().filter(|r| matches!(r, CoreRequirement::Any)).count();
        let specific_cores: Vec<BioCore> = spec
            .cores
            .iter()
            .filter_map(|r| if let CoreRequirement::Specific(c) = r { Some(*c) } else { None })
            .collect();

        let available_cores: Vec<usize> = (0..4)
            .filter(|i| self.cores[*i].is_none() && !specific_cores.contains(&BioCore::from(*i)))
            .collect();

        if any_count > available_cores.len() {
            return Err(ResourceError::InsufficientCores {
                requested: any_count,
                available: available_cores.len(),
            });
        }

        // Check FIFOs
        for fifo in &spec.fifos {
            if let Some(claimer) = &self.fifos[*fifo as usize] {
                return Err(ResourceError::FifoUnavailable { fifo: *fifo, claimed_by: claimer.clone() });
            }
        }

        // Check static pins
        for pin in &spec.static_pins {
            if let Some(claimer) = &self.pins[*pin as usize] {
                return Err(ResourceError::PinUnavailable { pin: *pin, claimed_by: claimer.clone() });
            }
        }

        // Check dynamic pin capacity
        let current_dynamic: u8 = self.dynamic_reservations.iter().map(|(_, n)| n).sum();
        let current_static: u8 = self.pins.iter().filter(|p| p.is_some()).count() as u8;
        if current_static + current_dynamic + spec.dynamic_pin_count > 32 {
            return Err(ResourceError::DynamicPinOverflow {
                requested: spec.dynamic_pin_count,
                already_reserved: current_dynamic,
                static_pins_used: current_static,
            });
        }

        Ok(())
    }

    /// Check if multiple specs can coexist (ignoring current state).
    pub fn check_specs_batch(&self, specs: &[ResourceSpec]) -> Result<(), ResourceError> {
        // Build a temporary tracker starting from current state
        let mut temp = self.clone();

        for spec in specs {
            temp.check_spec(spec)?;
            // Temporarily "claim" resources to check for conflicts between specs
            temp.claim_spec_internal(spec)?;
        }

        Ok(())
    }

    /// Actually claim resources. Returns the grant on success.
    pub fn claim(&mut self, spec: &ResourceSpec) -> Result<ResourceGrant, ResourceError> {
        // First check
        self.check_spec(spec)?;

        // Now actually allocate
        let grant = self.claim_spec_internal(spec)?;

        // Record the grant
        self.active_grants.push((grant.grant_id, spec.claimer.clone()));

        Ok(grant)
    }

    fn claim_spec_internal(&mut self, spec: &ResourceSpec) -> Result<ResourceGrant, ResourceError> {
        let mut allocated_cores = Vec::new();

        // Allocate specific cores first
        for req in &spec.cores {
            if let CoreRequirement::Specific(core) = req {
                self.cores[*core as usize] = Some(spec.claimer.clone());
                allocated_cores.push(*core);
            }
        }

        // Allocate "Any" cores
        for req in &spec.cores {
            if matches!(req, CoreRequirement::Any) {
                // Find first available core not already allocated to this spec
                for i in 0..4 {
                    if self.cores[i].is_none() {
                        self.cores[i] = Some(spec.claimer.clone());
                        allocated_cores.push(BioCore::from(i));
                        break;
                    }
                }
            }
        }

        // Allocate FIFOs
        for fifo in &spec.fifos {
            self.fifos[*fifo as usize] = Some(spec.claimer.clone());
        }

        // Allocate static pins
        for pin in &spec.static_pins {
            self.pins[*pin as usize] = Some(spec.claimer.clone());
        }

        // Record dynamic pin reservation
        if spec.dynamic_pin_count > 0 {
            self.dynamic_reservations.push((spec.claimer.clone(), spec.dynamic_pin_count));
        }

        let grant_id = self.next_grant_id;
        self.next_grant_id += 1;

        Ok(ResourceGrant {
            grant_id,
            cores: allocated_cores,
            fifos: spec.fifos.clone(),
            static_pins: spec.static_pins.clone(),
            dynamic_pin_count: spec.dynamic_pin_count,
        })
    }

    /// Release a grant by ID
    pub fn release(&mut self, grant_id: u32) -> Result<(), ResourceError> {
        // Find the grant
        let pos = self.active_grants.iter().position(|(id, _)| *id == grant_id);
        let claimer = match pos {
            Some(p) => {
                let (_, claimer) = self.active_grants.remove(p);
                claimer
            }
            None => return Err(ResourceError::InvalidGrantId(grant_id)),
        };

        // Release all resources claimed by this claimer
        for core in &mut self.cores {
            if core.as_ref() == Some(&claimer) {
                *core = None;
            }
        }
        for fifo in &mut self.fifos {
            if fifo.as_ref() == Some(&claimer) {
                *fifo = None;
            }
        }
        for pin in &mut self.pins {
            if pin.as_ref() == Some(&claimer) {
                *pin = None;
            }
        }
        self.dynamic_reservations.retain(|(c, _)| c != &claimer);

        Ok(())
    }

    /// Claim a specific dynamic pin
    pub fn claim_dynamic_pin(&mut self, pin: u8, claimer: &str) -> Result<(), ResourceError> {
        if pin >= 32 {
            return Err(ResourceError::InternalError);
        }

        // Check if this claimer has dynamic reservation
        let has_reservation = self.dynamic_reservations.iter().any(|(c, _)| c == claimer);
        if !has_reservation {
            return Err(ResourceError::ClaimerMismatch {
                expected: "a claimer with dynamic pin reservation".to_string(),
                provided: claimer.to_string(),
            });
        }

        // Check if pin is available
        if let Some(current) = &self.pins[pin as usize] {
            return Err(ResourceError::PinUnavailable { pin, claimed_by: current.clone() });
        }

        // Claim it
        self.pins[pin as usize] = Some(claimer.to_string());

        // Decrement dynamic reservation
        for (c, count) in &mut self.dynamic_reservations {
            if c == claimer && *count > 0 {
                *count -= 1;
                break;
            }
        }

        Ok(())
    }

    /// Release a specific dynamic pin
    pub fn release_dynamic_pin(&mut self, pin: u8, claimer: &str) -> Result<(), ResourceError> {
        if pin >= 32 {
            return Err(ResourceError::InternalError);
        }

        match &self.pins[pin as usize] {
            Some(current) if current == claimer => {
                self.pins[pin as usize] = None;
                // Restore dynamic reservation count
                for (c, count) in &mut self.dynamic_reservations {
                    if c == claimer {
                        *count += 1;
                        break;
                    }
                }
                Ok(())
            }
            Some(other) => {
                Err(ResourceError::ClaimerMismatch { expected: other.clone(), provided: claimer.to_string() })
            }
            None => Err(ResourceError::InternalError),
        }
    }
}

impl Clone for ResourceTracker {
    fn clone(&self) -> Self {
        Self {
            cores: self.cores.clone(),
            fifos: self.fifos.clone(),
            pins: self.pins.clone(),
            dynamic_reservations: self.dynamic_reservations.clone(),
            next_grant_id: self.next_grant_id,
            active_grants: self.active_grants.clone(),
        }
    }
}
