use packing::{
    Packed,
    PackedSize,
    Error as PackingError,
};
use usb_device::class_prelude::*;
use usb_device::Result as UsbResult;

use usbd_bulk_only_transport::{
    BulkOnlyTransport,
    Error as BulkOnlyTransportError,
    TransferState,
};

use usbd_mass_storage::InterfaceSubclass;

use crate::{
    logging::*,
    block_device::{
        BlockDevice,
        BlockDeviceError,
    },
    scsi::{
        commands::*,
        responses::*,
        enums::*,
        Error,
    },
};

enum CommandState {
    None,
    Done,
    Ongoing,
}

/// # Scsi Transparent Command Set implementation
///
/// Built on top of [BulkOnlyTransport](struct.BulkOnlyTransport.html)
///
/// [Glossary](index.html#glossary)
pub struct Scsi<'a, B: UsbBus, BD: BlockDevice> {
    inner: BulkOnlyTransport<'a, B>,
    current_command: Command,
    inquiry_response: InquiryResponse,
    request_sense_response: RequestSenseResponse,
    block_device: BD,
    lba: u32,
    lba_end: u32,
}

impl<B: UsbBus, BD: BlockDevice> Scsi<'_, B, BD> {
    /// Creates a new Scsi block device
    ///
    /// `block_device` provides reading and writing of blocks to the underlying filesystem
    ///
    /// `vendor_identification` is an ASCII string that forms part of the SCSI inquiry response. 
    ///      Should come from [t10](https://www.t10.org/lists/2vid.htm). Any semi-unique non-blank
    ///      string should work fine for local development. Panics if > 8 characters are supplied.
    ///
    /// `product_identification` is an ASCII string that forms part of the SCSI inquiry response. 
    ///      Vendor (probably you...) defined so pick whatever you want. Panics if > 16 characters
    ///      are supplied.
    ///
    /// `product_revision_level` is an ASCII string that forms part of the SCSI inquiry response. 
    ///      Vendor (probably you...) defined so pick whatever you want. Typically a version number.
    ///      Panics if > 4 characters are supplied.
    pub fn new<V: AsRef<[u8]>, P: AsRef<[u8]>, R: AsRef<[u8]>> (
        alloc: &UsbBusAllocator<B>, 
        max_packet_size: u16, 
        block_device: BD,
        vendor_identification: V,
        product_identification: P,
        product_revision_level: R,
    ) -> Scsi<'_, B, BD> {
        let mut inquiry_response = InquiryResponse::default();
        inquiry_response.set_vendor_identification(vendor_identification);
        inquiry_response.set_product_identification(product_identification);
        inquiry_response.set_product_revision_level(product_revision_level);

        //TODO: This is reasonable for FAT but not FAT32 or others. BOT buffer should probably be 
        //configurable from here, perhaps passing in BD::BLOCK_BYTES.max(BOT::MIN_BUFFER) or something 
        assert!(BD::BLOCK_BYTES <= BulkOnlyTransport::<B>::BUFFER_BYTES);
        Scsi {
            inner: BulkOnlyTransport::new(
                alloc, 
                max_packet_size, 
                InterfaceSubclass::ScsiTransparentCommandSet,
                0,
            ),
            current_command: Command::None,
            inquiry_response,
            request_sense_response: Default::default(),
            block_device,
            lba: 0,
            lba_end: 0,
        }
    }

    /// Grants access to the block device for the purposes of housekeeping etc.
    pub fn block_device_mut(&mut self) -> &mut BD {
        &mut self.block_device
    }

    fn get_new_command(&mut self) -> Result<bool, Error> {
        if self.current_command != Command::None {
            Ok(false)
        } else {
            if let Some(cbw) = self.inner.get_current_command() {
                self.current_command = Command::extract_from_cbw(cbw)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    fn process_command(&mut self, new_command: bool) -> Result<CommandState, Error> {
        use CommandState::*;

        trace_scsi_command!("COMMAND> {:?}", self.current_command);

        Ok(match self.current_command {
            // No command, nothing to do
            Command::None => None,

            // Inquiry, send back inquiry response
            // TODO: This always responds with "standard" response data but the req might be
            // for descriptor based response data.
            Command::Inquiry(_) => {
                let buf = self.inner.take_buffer_space(InquiryResponse::BYTES)?;
                self.inquiry_response.pack(buf)?;
                Done
            },

            // Testing if the unit is ready. Responding CommandOk is sufficient for this request
            // TODO: There's a situation where after enough errors the host will keep sending TUR
            // requests and nothing else, regardless of the response. There may be additional data
            // in the request that indicates you should respond CommandError and prepare sense
            // response data with more info but not looked in detail yet.
            Command::TestUnitReady(_) => Done,

            // Prevent the user removing the disk, not implemented. Just responding CommandOk sufficient
            // for flash based device.
            Command::PreventAllowMediumRemoval(_) => Done,

            // Read the capacity and block size of the device
            Command::ReadCapacity(_)  => {
                let max_lba = self.block_device.max_lba();
                let block_size = BD::BLOCK_BYTES as u32;
                let cap = ReadCapacity10Response {
                    max_lba,
                    block_size,
                };
                
                let buf = self.inner.take_buffer_space(ReadCapacity10Response::BYTES)?;
                cap.pack(buf)?;
                Done
            },

            // Check the readonly and cache (potentially other info) about the device
            Command::ModeSense(ModeSenseXCommand { command_length: CommandLength::C6, page_control: PageControl::CurrentValues })  => {
                let mut header = ModeParameterHeader6::default();
                header.increase_length_for_page(PageCode::CachingModePage);
                
                // Default is both caches disabled
                let cache_page = CachingModePage::default();

                let buf = self.inner.take_buffer_space(
                    ModeParameterHeader6::BYTES + CachingModePage::BYTES
                )?;   

                header.pack(&mut buf[..ModeParameterHeader6::BYTES])?;
                cache_page.pack(&mut buf[ModeParameterHeader6::BYTES..])?;
                Done
            },

            // Request sense is how more info about the state of the device is returned
            // Returning CommandError will cause the host to perform a request sense
            // to get more details.
            Command::RequestSense(_) => {
                let buf = self.inner.take_buffer_space(RequestSenseResponse::BYTES)?;
                self.request_sense_response.pack(buf)?;
                Done
            },

            // Read `transfer_length` blocks from `lba`
            Command::Read(r) => {
                // Record the end condition
                if new_command {
                    self.lba = r.lba;
                    self.lba_end = r.lba + r.transfer_length - 1;
                }

                trace_scsi_fs!("FS> Read; new: {}, lba: 0x{:X?}, lba_end: 0x{:X?}, done: {}",
                    new_command, self.lba, self.lba_end, self.lba == self.lba_end);

                // We only get here if the buffer is empty 
                let buf = self.inner.take_buffer_space(BD::BLOCK_BYTES)?;
                self.block_device.read_block(self.lba, buf)?;
                self.lba += 1;

                if self.lba <= self.lba_end {
                    Ongoing
                } else {
                    Done
                }
            },

            // Write `transfer_length` blocks from `lba`
            Command::Write(w) => {
                // Record the end condition
                if new_command {
                    self.lba = w.lba;
                    self.lba_end = w.lba + w.transfer_length - 1;
                }

                trace_scsi_fs!("FS> Write; new: {}, lba: 0x{:X?}, lba_end: 0x{:X?}, done: {}",
                    new_command, self.lba, self.lba_end, self.lba == self.lba_end);

                let len = match self.inner.transfer_state() {
                    TransferState::ReceivingDataFromHost { done: true, full: false, bytes_available: b } => b,
                    // TODO: Does this ever happen?
                    _ => BD::BLOCK_BYTES,
                };

                let buf = self.inner.take_buffered_data(len, false).expect("Buffer should have enough data");
                self.block_device.write_block(self.lba, buf)?;
                self.lba += 1;

                if self.lba <= self.lba_end {
                    Ongoing
                } else {
                    Done
                }
            },

            _ => Err(Error::UnhandledOpCode)?,
        })
    }

    fn receive_command(&mut self) -> Result<(), Error> {
        let transfer_state = self.inner.transfer_state();
        // These calls all assume only a single block will fit in the buffer which 
        // is true here because we configure BOT that way but we could make the inner
        // buffer length a multiple of BLOCK_SIZE and queue up more than one block
        // at a time. I don't know if there's any benefit to that but the option is there
        let skip = match transfer_state {
            TransferState::ReceivingDataFromHost { full, done, .. } => {
                !(full || done)
            },
            TransferState::SendingDataToHost { empty, .. } => {
                !empty
            },
            // We still need to check if the buffer is empty because if a CSW is being sent
            // we won't be able to grab a full block buffer if the next command happens to be
            // a Read
            TransferState::NotTransferring { empty, .. } => {
                !empty
            }
        };

        if skip {
            Err(UsbError::WouldBlock)?;
        }

        let new_command = self.get_new_command()?;

        match self.process_command(new_command) {
            Ok(CommandState::Done) => {
                // Command is done, send CommandOk
                self.inner.send_command_ok()?;
                // Clear the command so we don't try and execute it again
                self.current_command = Command::None;

                // Reset sense code to good
                self.request_sense_response.reset_status();
            },
            // WouldBlock error is handled the same as ongoing (i.e. do nothing)
            Ok(CommandState::None) |
            Ok(CommandState::Ongoing) |
            Err(Error::BulkOnlyTransportError(
                BulkOnlyTransportError::UsbError(
                    UsbError::WouldBlock))) => {
                // No command, command is ongoing or we couldn't get a buffer/some other WouldBlock issue
                // Do nothing
            },
            Err(e) => {
                // Command failed, send CommandErr
                self.inner.send_command_error()?;
                // Clear the command so we don't try and execute it again
                // All errors immediately terminate the command and cause the host to
                // retry or issue RequestSense to find out more info
                self.current_command = Command::None;

                // Update the sense data so the host can find out what went wrong
                self.map_error_to_sense_data(&e);

                // Return the error to the caller so it can get logged
                Err(e)?;
            },
        }

        Ok(())
    }

    fn map_error_to_sense_data(&mut self, err: &Error) {
        let (sense_key, additional_sense_code) = match err {
            Error::UnhandledOpCode => (
                 SenseKey::IllegalRequest,
                 AdditionalSenseCode::InvalidCommandOperationCode,
            ),
            
            Error::InsufficientDataForCommand => (
                SenseKey::IllegalRequest,
                // Closest thing I could find. Some sources suggest OS does very little with ASC/ASCQ and it's
                // most useful for debugging so as long as it's unique here it's probably ok.
                AdditionalSenseCode::InvalidPacketSize,
            ),

            Error::PackingError(p) |
            Error::BulkOnlyTransportError(BulkOnlyTransportError::PackingError(p)) => match p {
                PackingError::InsufficientBytes => panic!("PackingError::InsufficientBytes: Logical error in program"),
                PackingError::Infallible(_) => unreachable!(),
                PackingError::InvalidEnumDiscriminant => (
                    SenseKey::IllegalRequest,
                    AdditionalSenseCode::InvalidFieldInCdb,
                ),
            },

            Error::BlockDeviceError(BlockDeviceError::HardwareError) => (
                SenseKey::HardwareError,
                //TODO: split errors up more?
                AdditionalSenseCode::NoAdditionalSenseInformation,
            ),
            Error::BlockDeviceError(BlockDeviceError::WriteError) => (
                SenseKey::MediumError,
                AdditionalSenseCode::WriteError,
            ),
            Error::BlockDeviceError(BlockDeviceError::EraseError) => (
                SenseKey::MediumError,
                AdditionalSenseCode::EraseFailure,
            ),
            Error::BlockDeviceError(BlockDeviceError::InvalidAddress) => (
                SenseKey::IllegalRequest,
                AdditionalSenseCode::LogicalBlockAddressOutOfRange,
            ),

            Error::BulkOnlyTransportError(BulkOnlyTransportError::DataError) => (
                SenseKey::IllegalRequest,
                AdditionalSenseCode::InvalidFieldInCdb,
            ),

            // These USB errors are likely to result in a USB reset, it's unlikely a SCSI
            // request sense will ever be issued in these cases but just-in-case
            Error::BulkOnlyTransportError(BulkOnlyTransportError::UsbError(_)) => (
                SenseKey::HardwareError,
                AdditionalSenseCode::NoAdditionalSenseInformation,
            ),
        };

        info!("SENSE: {:?}, ASC: {} {}", sense_key, additional_sense_code.asc(), additional_sense_code.ascq());
        self.request_sense_response.sense_key = sense_key;
        self.request_sense_response.additional_sense_code = additional_sense_code;
    }

    fn update(&mut self) -> Result<(), Error> {

        // Send anything that's already queued
        accept_would_block(
            self.inner.write()
                .map_err(|e| e.into())
        )?;

        // Read new data if available
        accept_would_block(
            self.inner.read()
                .map_err(|e| e.into())
        )?;

        // Recieve and execute a command if one is available
        accept_would_block(self.receive_command())?;

        // Send anything we may have generated this go around
        accept_would_block(
            self.inner.write()
                .map_err(|e| e.into())
        )?;

        Ok(())
    }
}

fn accept_would_block(r: Result<(), Error>) -> Result<(), Error> {
    match r {
        Ok(_) | Err(Error::BulkOnlyTransportError(BulkOnlyTransportError::UsbError(UsbError::WouldBlock))) => Ok(()),
        e => e
    }
}

impl<B: UsbBus, BD: BlockDevice> UsbClass<B> for Scsi<'_, B, BD> {
    fn get_configuration_descriptors(&self, writer: &mut DescriptorWriter) -> UsbResult<()> {
        self.inner.get_configuration_descriptors(writer)
    }

    fn reset(&mut self) { 
        self.current_command = Command::None;
        self.request_sense_response.reset_status();
        self.lba = 0;
        self.lba_end = 0;

        self.inner.reset()
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        self.inner.control_in(xfer)
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        self.inner.control_out(xfer)
    }

    fn poll(&mut self) { 
        if let Err(e) = self.update() {
            error!("Error from Scsi::update: {:?}", e);
        }
    }
}