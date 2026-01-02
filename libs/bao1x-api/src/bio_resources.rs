use crate::bio::*;

/// Specifies a core requirement
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum CoreRequirement {
    /// Any available core will do
    Any,
    /// A specific core is required
    Specific(BioCore),
}

/// Specification of resources an application needs
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ResourceSpec {
    /// Human-readable identifier for the claimer (for debugging/error messages)
    pub claimer: String,
    /// Core requirements - each element represents one core needed
    pub cores: Vec<CoreRequirement>,
    /// Specific FIFOs required (always specific, never "any")
    pub fifos: Vec<Fifo>,
    /// Statically known pins (by pin number 0-31)
    pub static_pins: Vec<u8>,
    /// Number of dynamically allocated pins (best-effort tracking only)
    pub dynamic_pin_count: u8,
}

#[cfg(feature = "std")]
impl ResourceSpec {
    pub fn new(claimer: impl Into<String>) -> Self {
        Self {
            claimer: claimer.into(),
            cores: Vec::new(),
            fifos: Vec::new(),
            static_pins: Vec::new(),
            dynamic_pin_count: 0,
        }
    }

    pub fn any_core(mut self) -> Self {
        self.cores.push(CoreRequirement::Any);
        self
    }

    pub fn any_cores(mut self, count: usize) -> Self {
        for _ in 0..count {
            self.cores.push(CoreRequirement::Any);
        }
        self
    }

    pub fn specific_core(mut self, core: BioCore) -> Self {
        self.cores.push(CoreRequirement::Specific(core));
        self
    }

    pub fn fifo(mut self, fifo: Fifo) -> Self {
        if !self.fifos.contains(&fifo) {
            self.fifos.push(fifo);
        }
        self
    }

    pub fn pin(mut self, pin: u8) -> Self {
        assert!(pin < 32, "Pin must be 0-31");
        if !self.static_pins.contains(&pin) {
            self.static_pins.push(pin);
        }
        self
    }

    pub fn pins_from_mask(mut self, mask: u32) -> Self {
        for i in 0..32u8 {
            if mask & (1 << i) != 0 && !self.static_pins.contains(&i) {
                self.static_pins.push(i);
            }
        }
        self
    }

    pub fn dynamic_pins(mut self, count: u8) -> Self {
        self.dynamic_pin_count = count;
        self
    }
}

/// Result of a successful resource claim
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ResourceGrant {
    /// Unique ID for this grant (used for release)
    pub grant_id: u32,
    /// The actual cores allocated, in the same order as requested
    pub cores: Vec<BioCore>,
    /// The FIFOs granted (mirrors request, confirms allocation)
    pub fifos: Vec<Fifo>,
    /// Static pins granted
    pub static_pins: Vec<u8>,
    /// Dynamic pin capacity reserved
    pub dynamic_pin_count: u8,
}

/// Current state of resource availability (for diagnostics)
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ResourceAvailability {
    /// For each core: None if available, Some(claimer_name) if claimed
    pub cores: [Option<String>; 4],
    /// For each FIFO: None if available, Some(claimer_name) if claimed
    pub fifos: [Option<String>; 4],
    /// For each of 32 pins: None if available, Some(claimer_name) if claimed
    pub pins: [Option<String>; 32],
    /// Total dynamic pins reserved across all claimers
    pub dynamic_pins_reserved: u8,
}

#[cfg(feature = "std")]
impl Default for ResourceAvailability {
    fn default() -> Self {
        // Ugly but necessary since Option<String> doesn't impl Copy
        Self {
            cores: [None, None, None, None],
            fifos: [None, None, None, None],
            pins: [
                None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None,
            ],
            dynamic_pins_reserved: 0,
        }
    }
}

/// Detailed error for resource conflicts
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ResourceError {
    /// No error (used as sentinel in IPC)
    None,
    /// A specific core was requested but is unavailable
    CoreUnavailable { core: BioCore, claimed_by: String },
    /// Not enough free cores for "Any" requests
    InsufficientCores { requested: usize, available: usize },
    /// A FIFO is unavailable
    FifoUnavailable { fifo: Fifo, claimed_by: String },
    /// A pin is unavailable
    PinUnavailable { pin: u8, claimed_by: String },
    /// Too many dynamic pins would be reserved (exceeds 32 total)
    DynamicPinOverflow { requested: u8, already_reserved: u8, static_pins_used: u8 },
    /// Grant ID not found (already released or invalid)
    InvalidGrantId(u32),
    /// Claimer name mismatch on dynamic pin operation
    ClaimerMismatch { expected: String, provided: String },
    /// Internal error
    InternalError,
}

impl From<ResourceError> for BioError {
    fn from(e: ResourceError) -> Self {
        match e {
            ResourceError::None => BioError::None,
            ResourceError::InternalError => BioError::InternalError,
            _ => BioError::ResourceInUse,
        }
    }
}

// ============================================================================
// IPC MESSAGE WRAPPERS
// ============================================================================

/// IPC wrapper for claim_resources / check_resources
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ClaimResourcesRequest {
    pub spec: ResourceSpec,
    /// On return: the grant (if successful)
    pub grant: Option<ResourceGrant>,
    /// On return: the error (if failed)
    pub error: ResourceError,
}

/// IPC wrapper for check_resources_batch
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CheckResourcesBatchRequest {
    pub specs: Vec<ResourceSpec>,
    /// On return: the error (if any conflict found)
    pub error: ResourceError,
}

/// IPC wrapper for resource_availability query
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ResourceAvailabilityResponse {
    pub availability: ResourceAvailability,
}

/// IPC wrapper for dynamic pin operations
#[derive(Clone, Debug)]
#[cfg_attr(feature = "std", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DynamicPinRequest {
    pub pin: u8,
    pub claimer: String,
    /// On return: the error (if failed)
    pub error: ResourceError,
}

/// Resource management API for BIO applications.
///
/// This trait is only available with the `std` feature, as it requires
/// communication with the global BIO resource server.
///
/// In no-std/baremetal environments, resource tracking is the programmer's
/// responsibility.
///
/// The `std`/`no-std` split in the API means that the resource tracking has to be
/// decoupled from the actual resource usage. In other words, all `std` drivers
/// have to first do a step of securing the resources, and then a step of actually
/// initializing them.
#[cfg(feature = "std")]
pub trait BioResources {
    /// Claim resources according to spec.
    fn claim_resources(&self, spec: &ResourceSpec) -> Result<ResourceGrant, ResourceError>;

    /// Release previously claimed resources.
    fn release_resources(&self, grant_id: u32) -> Result<(), ResourceError>;

    /// Query current resource availability.
    fn resource_availability(&self) -> Result<ResourceAvailability, BioError>;

    /// Check if a spec could be satisfied without actually claiming.
    fn check_resources(&self, spec: &ResourceSpec) -> Result<(), ResourceError>;

    /// Check if multiple specs can coexist (ignoring current state).
    fn check_resources_batch(&self, specs: &[ResourceSpec]) -> Result<(), ResourceError>;

    /// Claim a specific dynamic pin at runtime.
    fn claim_dynamic_pin(&self, pin: u8, claimer: &str) -> Result<(), ResourceError>;

    /// Release a specific dynamic pin.
    fn release_dynamic_pin(&self, pin: u8, claimer: &str) -> Result<(), ResourceError>;

    /// Sets the run state of the cores based on the resource grant. Starts the cores if `start` is `true`,
    /// otherwise stops them.
    fn set_core_run_state(&self, grant: &ResourceGrant, start: bool);
}

pub trait Resources {
    fn resource_spec() -> ResourceSpec;
}
