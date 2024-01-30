use std::sync::Arc;
use std::{collections::VecDeque, sync::Mutex};

use frunk_core::hlist::{HCons, HNil};
use fugit::ExtU32;
use usb_device::device::UsbDeviceBuilder;
use usb_device::{
    class_prelude::{UsbBus, UsbBusAllocator},
    device::{UsbDevice, UsbVidPid},
    UsbError,
};
use usb_device_xous::UsbDeviceState;
use xous_usb_hid::{
    device::DeviceClass,
    interface::{
        HeapInterface, HeapInterfaceBuilder, HeapInterfaceConfig, InBytes64, OutBytes64, ReportSingle,
        UsbAllocatable,
    },
    usb_class::{UsbHidClass, UsbHidClassBuilder},
    UsbHidError,
};

use crate::api::HIDReport;

/// AppHIDDevice implements a UsbBus device whose internal USB interface allows for a heap-allocated
/// device report descriptor.
pub struct AppHIDDevice<'a, B: UsbBus> {
    interface: HeapInterface<'a, B, InBytes64, OutBytes64, ReportSingle>,
}

impl<'a, B: UsbBus> AppHIDDevice<'a, B> {
    pub fn write_report(&mut self, report: &HIDReport) -> Result<(), UsbHidError> {
        self.interface.write_report(&report.0).map(|_| ()).map_err(UsbHidError::from)
    }

    pub fn read_report(&mut self) -> usb_device::Result<HIDReport> {
        let mut report = HIDReport::default();
        match self.interface.read_report(&mut report.0) {
            Err(e) => Err(e),
            Ok(_) => Ok(report),
        }
    }

    pub fn set_device_descriptor(&mut self, descriptor: Vec<u8>) -> Result<(), usb_device::UsbError> {
        self.interface.set_report_descriptor(descriptor)
    }
}

impl<'a, B: UsbBus> DeviceClass<'a> for AppHIDDevice<'a, B> {
    type I = HeapInterface<'a, B, InBytes64, OutBytes64, ReportSingle>;

    fn interface(&mut self) -> &mut Self::I { &mut self.interface }

    fn reset(&mut self) {}

    fn tick(&mut self) -> Result<(), UsbHidError> { Ok(()) }
}

pub struct AppHIDConfig<'a> {
    interface: HeapInterfaceConfig<'a, InBytes64, OutBytes64, ReportSingle>,
}

impl<'a> Default for AppHIDConfig<'a> {
    #[must_use]
    fn default() -> Self {
        let iface = HeapInterfaceBuilder::new(vec![0])
            .unwrap()
            .description("Xous HID application")
            .in_endpoint(5.millis())
            .unwrap()
            .with_out_endpoint(5.millis())
            .unwrap()
            .build();

        Self::new(iface)
    }
}

impl<'a> AppHIDConfig<'a> {
    #[must_use]
    pub fn new(interface: HeapInterfaceConfig<'a, InBytes64, OutBytes64, ReportSingle>) -> Self {
        Self { interface }
    }
}

impl<'a, B: UsbBus + 'a> UsbAllocatable<'a, B> for AppHIDConfig<'a> {
    type Allocated = AppHIDDevice<'a, B>;

    fn allocate(self, usb_alloc: &'a UsbBusAllocator<B>) -> Self::Allocated {
        Self::Allocated { interface: HeapInterface::new(usb_alloc, self.interface) }
    }
}

#[derive(Debug)]
pub enum AppHIDError {
    /// Returned when the maximum amount of packets allowed to stay in AppHID is reached, and the
    /// oldest is dropped.
    OldestReportDropped,

    /// No descriptor has been set on the AppHID, cannot poll.
    NoDescriptorSet,

    /// Returned when there is an error coming from the USB bus.
    UsbError(UsbError),
}

/// AppHID wraps a USB HID device, and allows for dynamic reconfiguration of the HID device descriptor.
/// It buffers incoming HID reports in a queue until it is requested to return some.
/// Developers can set the maximum amount of HID reports to buffer, and AppHID will drop the oldest when a new
/// one is added, and threshold is met.
/// The buffer is heap-allocated: one HID report is 64 bytes, so depending on your configuration you might
/// need to increase the heap limits for your process.
pub struct AppHID<'a, B: UsbBus> {
    max_stored_reports: usize,
    incoming_reports: VecDeque<HIDReport>,
    device_descr_set: Arc<Mutex<bool>>,

    class: UsbHidClass<'a, B, HCons<AppHIDDevice<'a, B>, HNil>>,
    device: UsbDevice<'a, B>,
}

impl<'a, B: UsbBus> AppHID<'a, B> {
    /// Creates a new USB device and HID class with the given parameters.
    pub fn new(
        vid_pid: UsbVidPid,
        serial_number: &'a str,
        alloc: &'a UsbBusAllocator<B>,
        config: AppHIDConfig<'a>,
        max_stored_reports: usize,
    ) -> Self {
        let class = UsbHidClassBuilder::new().add_device(config).build(alloc);

        let device = UsbDeviceBuilder::new(alloc, vid_pid)
            .manufacturer("Kosagi")
            .product("Precursor")
            .serial_number(&serial_number)
            .build();

        AppHID {
            max_stored_reports,
            incoming_reports: VecDeque::new(),
            device_descr_set: Arc::new(Mutex::new(false)),
            class,
            device,
        }
    }

    /// Forces the reset of the underlying USB device.
    /// Causes host to re-enumerate.
    pub fn force_reset(&mut self) -> usb_device::Result<()> { self.device.force_reset() }

    /// Returns the current state of the underlying USB device.
    pub fn state(&self) -> UsbDeviceState { self.device.state() }

    /// Polls for new reports on the bus, sent from the USB host.
    pub fn poll(&mut self) -> Result<(), AppHIDError> {
        if !self.device.poll(&mut [&mut self.class]) {
            return Ok(());
        }

        if !self.descriptor_set() {
            return Err(AppHIDError::NoDescriptorSet);
        }

        let hidv2_device = self.class.device::<AppHIDDevice<'_, _>, _>();
        match hidv2_device.read_report() {
            Ok(report) => {
                let result = 'result_scope: {
                    let reports_stored = self.incoming_reports.len();
                    if reports_stored < self.max_stored_reports {
                        break 'result_scope Ok(());
                    }

                    self.incoming_reports.pop_front().unwrap();
                    break 'result_scope Err(AppHIDError::OldestReportDropped);
                };

                self.incoming_reports.push_back(report);
                result
            }
            Err(err) => {
                // If we have something that isn't WouldBlock, maybe stuff's about to blow up: return to
                // caller.
                if !matches!(err, UsbError::WouldBlock) {
                    return Err(AppHIDError::UsbError(err));
                }

                return Ok(());
            }
        }
    }

    /// Sets the device descriptor report, which will be then sent to the host.
    /// This method forces a device reset, hence re-enumeration from the host.
    pub fn set_device_report(&mut self, descriptor: Vec<u8>) -> Result<(), AppHIDError> {
        let descr_len = descriptor.len();
        let hidv2_device = self.class.device::<AppHIDDevice<'_, _>, _>();
        hidv2_device.set_device_descriptor(descriptor).map_err(|e| AppHIDError::UsbError(e)).and_then(|_| {
            *self.device_descr_set.lock().unwrap() = match descr_len {
                0 => false,
                _ => true,
            };

            self.device.force_reset().ok();

            Ok(())
        })
    }

    /// Resets the stored device descriptor report and drops all the stored incoming reports, if any.
    /// This method forces a device reset, hence re-enumeration from the host.
    pub fn reset_device_report(&mut self) -> Result<(), AppHIDError> {
        self.incoming_reports.clear();
        self.set_device_report(vec![])
    }

    /// Returns true if there is a device descriptor set for the AppHID.
    pub fn descriptor_set(&self) -> bool { *self.device_descr_set.lock().unwrap() }

    /// Returns the oldest HID report read from the USB bus.
    pub fn read_report(&mut self) -> Option<HIDReport> {
        if !self.descriptor_set() {
            return None;
        }

        self.incoming_reports.pop_front()
    }

    /// Writes a HID report on the USB bus.
    pub fn write_report(&mut self, data: HIDReport) {
        let hidv2_device = self.class.device::<AppHIDDevice<'_, _>, _>();
        hidv2_device.write_report(&data).ok();
    }
}
