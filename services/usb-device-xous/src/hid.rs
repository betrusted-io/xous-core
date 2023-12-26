use usb_device::class_prelude::{UsbBus, UsbBusAllocator};
use xous_usb_hid::{
    device::DeviceClass,
    interface::{
        HeapInterface, HeapInterfaceBuilder, HeapInterfaceConfig, InBytes64, OutBytes64,
        ReportSingle, UsbAllocatable,
    },
    UsbHidError,
};

use crate::api::HIDReport;
use fugit::ExtU32;

pub struct AppHID<'a, B: UsbBus> {
    interface: HeapInterface<'a, B, InBytes64, OutBytes64, ReportSingle>,
}

impl<'a, B: UsbBus> AppHID<'a, B> {
    pub fn write_report(&mut self, report: &HIDReport) -> Result<(), UsbHidError> {
        self.interface
            .write_report(&report.0)
            .map(|_| ())
            .map_err(UsbHidError::from)
    }

    pub fn read_report(&mut self) -> usb_device::Result<HIDReport> {
        let mut report = HIDReport::default();
        match self.interface.read_report(&mut report.0) {
            Err(e) => Err(e),
            Ok(_) => Ok(report),
        }
    }

    pub fn set_device_descriptor(
        &mut self,
        descriptor: Vec<u8>,
    ) -> Result<(), usb_device::UsbError> {
        self.interface.set_report_descriptor(descriptor)
    }
}

impl<'a, B: UsbBus> DeviceClass<'a> for AppHID<'a, B> {
    type I = HeapInterface<'a, B, InBytes64, OutBytes64, ReportSingle>;

    fn interface(&mut self) -> &mut Self::I {
        &mut self.interface
    }

    fn reset(&mut self) {}

    fn tick(&mut self) -> Result<(), UsbHidError> {
        Ok(())
    }
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
    type Allocated = AppHID<'a, B>;

    fn allocate(self, usb_alloc: &'a UsbBusAllocator<B>) -> Self::Allocated {
        Self::Allocated {
            interface: HeapInterface::new(usb_alloc, self.interface),
        }
    }
}
