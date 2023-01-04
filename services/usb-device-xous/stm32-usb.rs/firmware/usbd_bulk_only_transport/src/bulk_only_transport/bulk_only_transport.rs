use usb_device::class_prelude::*;
use usb_device::{
    Result as UsbResult,
    control::{
        RequestType,
        Request,
    },
};
pub use UsbError::WouldBlock;

use packing::{
    Error as PackingError,
    Packed,
    PackedSize,
};

use usbd_mass_storage::{
    MscClass,
    InterfaceSubclass,
    InterfaceProtocol,
};
use crate::logging::*;
use super::{
    CommandBlockWrapper,
    CommandStatusWrapper,
    Direction,
    CommandStatus,
};


const REQ_GET_MAX_LUN: u8 = 0xFE;
const REQ_BULK_ONLY_RESET: u8 = 0xFF;

const BUFFER_BYTES: usize = 512;

#[derive(Debug)]
pub enum Error {
    UsbError(UsbError),
    PackingError(PackingError),
    DataError,
}

impl From<UsbError> for Error {
    fn from(e: UsbError) -> Error {
        Error::UsbError(e)
    }
}
impl From<PackingError> for Error {
    fn from(e: PackingError) -> Error {
        Error::PackingError(e)
    }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum State {
    /// Waiting for a command block wrapper to arrive. Throws away data until 
    /// CBW signature is found. Moves to SendingDataToHost or ReceivingDataFromHost
    /// depending on CBW direction flag
    WaitingForCommand,
    /// Command initiated a transfer to the host (IN in USB parlance). Sends the 
    /// number of bytes the command asked for unless instructed to terminate early. 
    /// Moves to NeedToSendStatus or NeedZlp
    SendingDataToHost,
    /// Command initiated a transfer from the host (OUT in USB parlance). Reads the 
    /// number of bytes the command asked to send. Moves to NeedToSendStatus or NeedZlp
    ReceivingDataFromHost,
    /// Need to send a zero length packet because the transfer was shorter than
    /// the requested length and we ended on a full packet. Sends a ZLP then moves to
    /// NeedToSendStatus
    NeedZlp,
    /// Data transfer has finished. Sends a command block status packet
    NeedToSendStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// There is no data transfer underway, we are either waiting for a command or
    /// about to send ZLP/CSW after a transfer has finished. We might be sending a CSW
    /// so `bytes_remaining` and `empty` use the same logic as `SendingDataToHost`
    NotTransferring { bytes_remaining: usize, empty: bool },
    /// We are receving data from the host, `bytes_available` indicates how many bytes
    /// have been read into the buffer. `full` indicates we're out of buffer space and
    /// need the caller to process the data before we can do more work. `done` indicates
    /// data_residue has reached 0 and there won't be any more data read
    ReceivingDataFromHost { bytes_available: usize, full: bool, done: bool },
    /// We are sending data to the host, `bytes_remaining` indicates how many bytes are 
    /// still in the buffer. `empty` indicates the buffer is empty and we can't do any
    /// more work before the caller gives us more data
    SendingDataToHost { bytes_remaining: usize, empty: bool },
}

/// # USB Bulk Only Transport protocol
///
/// So far only tested with the SCSI transparent command set - see [Scsi](struct.Scsi.html)
///
/// [Glossary](index.html#glossary)
/// 
/// ## Functionality overview
/// 1. Reading CBWs
/// 1. Initiating a data transfer with the length and direction from the CBW
/// 1. Sending USB packets to the underlaying driver when there is data in the buffer
/// 1. Terminating the data transfer when enough data is processed or early termination is requested
/// 1. Adding ZLP when necessary
/// 1. Sending CSW with correct data residue
/// 1. Responding to class specific control requests (bulk only reset and get max lun)
///
/// ## Unimplemented/Untested:
/// 1. More than 1 LUN - the SCSI implementation used for testing uses LUN 0 and responds to requests
///    for any other LUN with CommandError. This class will respond to get max lun correctly for any
///    valid value of `max_lun` but I haven't implemented anything that uses more than 1 LUN
/// 1. Bulk only mass storage reset that takes any length of time - the spec (Section 3.1 [USB Bulk Only Transport Spec](https://www.usb.org/document-library/mass-storage-bulk-only-10))
///    allows for the reset request to kick off the reset and have the host wait/poll until the reset
///    is done. This is likely for devices where the reset might take a long (relative to poll rate) time.
///    This isn't implemented here.
///
pub struct BulkOnlyTransport<'a, B: UsbBus> {
    inner: MscClass<'a, B>,
    
    /// This is the response this class will give to the Get Max LUN request
    /// must be between 0 and 15, which is checked in the new function
    max_lun: u8,

    /// Are we waiting, sending or receiving data
    state: State,

    /// The most recent CBW. Not valid if in WaitingForCommand state
    command_block_wrapper: CommandBlockWrapper,

    /// The previous command's CSW. 
    command_status_wrapper: CommandStatusWrapper,

    /// The buffer, used for both reading and writing
    buffer: [u8; BUFFER_BYTES],

    /// The next free index in the buffer
    buffer_i: usize,

    /// The next data to send in the buffer
    data_i: usize,

    /// Tracks if the last write was a full packet or not
    /// Used to determine if a ZLP is required or not
    last_packet_full: bool,

    /// Indicates we are going to end the current data transaction after draining the current 
    /// buffer regardless of data_residue
    data_done: bool,
}

impl<B: UsbBus> BulkOnlyTransport<'_, B> {
    pub const BUFFER_BYTES: usize = BUFFER_BYTES;
    pub fn new(
        alloc: &UsbBusAllocator<B>, 
        max_packet_size: u16, 
        subclass: InterfaceSubclass,
        max_lun: u8,
    ) -> BulkOnlyTransport<'_, B> {
        assert!(max_lun < 16);
        BulkOnlyTransport {
            inner: MscClass::new(
                alloc, 
                max_packet_size, 
                subclass,
                InterfaceProtocol::BulkOnlyTransport,
            ),
            max_lun,
            state: State::WaitingForCommand,
            command_block_wrapper: Default::default(),
            command_status_wrapper: Default::default(),
            buffer: [0; BUFFER_BYTES],
            buffer_i: 0,
            data_i: 0,
            last_packet_full: false,
            data_done: false,
        }
    }

    fn max_packet_size(&self) -> u16 {
        self.inner.max_packet_size()
    }

    fn max_packet_usize(&self) -> usize {
        self.max_packet_size() as usize
    }

    pub fn read(&mut self) -> Result<(), Error> {
        match self.state {
            State::WaitingForCommand => self.waiting_for_command(),
            State::ReceivingDataFromHost => self.receiving_data_from_host(),
            _ => Ok(()),
        }
    }

    pub fn write(&mut self) -> Result<(), Error> {
        match self.state {
            State::SendingDataToHost => self.sending_data_to_host(),
            State::NeedZlp => self.send_zlp(),
            State::NeedToSendStatus => self.need_to_send_status(),
            _ => Ok(()),
        }
    }

    fn waiting_for_command(&mut self) -> Result<(), Error> {
        if self.buffer.len() - self.buffer_i < (self.max_packet_size() as usize) {
            trace_bot_buffer!("BUFFER> too full to read command");
            Err(WouldBlock)?;
        }

        let bytes = self.inner.read_packet(&mut self.buffer[self.buffer_i..])?;
        trace_bot_bytes!("BYTES> Read {} bytes for command", bytes);
        self.buffer_i += bytes;
        // This looks for the signature and throws away any bytes until it finds one
        let new_i = CommandBlockWrapper::truncate_to_signature(&mut self.buffer[..self.buffer_i]);
        if self.buffer_i != new_i {
            trace_bot_headers!("HEADER> Discarded {} bytes looking for command block wrapper signature", self.buffer_i - new_i);
            self.buffer_i = new_i;
        }

        if self.buffer_i >= CommandBlockWrapper::BYTES {
            trace_bot_buffer!("BUFFER> full enough to try deserializing command block wrapper");
            let cbw = CommandBlockWrapper::unpack(&self.buffer)
                // We don't want to return a PackingError here because that's a fatal error
                // TODO: Possibly should set PhaseError?
                .map_err(|_| Error::DataError);
            if cbw.is_err() {
                let err = cbw.err().unwrap();
                warn!("CBW unpack error: {:?}", err);
                self.buffer_i = 0;
                return Err(err);
            }
            self.transition_to_data(cbw?);

            // After transitioning to data, we need to read but we might not get another interrupt
            // TODO: dig into this a bit. It seems that we *should* get an interrupt since the last
            // read that initiated the command should have drained the RX buffer. Depending on if
            // logging is enabled or not it appears to sometimes work and sometimes not which makes
            // me think it's a timing issue. In the hardware example I'm testing with RTFM appears
            // to be clearing the 
            self.read()?;
        }

        Ok(())
    }

    // Updates the command status wrapper in readiness to execute the provided command.
    // Copies the tag across, sets data_residue to the requested bytes and sets the 
    // status to OK.
    fn prepare_for_command(&mut self, cbw: &CommandBlockWrapper) {
        self.command_status_wrapper.tag = cbw.tag;
        self.command_status_wrapper.data_residue = cbw.data_transfer_length;
        self.command_status_wrapper.status = CommandStatus::CommandOk;
    }

    fn change_state(&mut self, new_state: State) {
        trace_bot_states!("STATE> {:?} -> {:?}",
            self.state,
            new_state,
        );
        self.state = new_state;
    }

    fn transition_to_data(&mut self, cbw: CommandBlockWrapper) {
        trace_bot_headers!("HEADER> CommandBlockWrapper: {:X?}", cbw);
        // Reset the positions in the buffer
        self.buffer_i = 0;
        self.data_i = 0;
        
        // Reset the data_done override
        self.data_done = false;

        // Update the csw so we can send that after the data
        self.prepare_for_command(&cbw);

        // Update the state
        match cbw.direction {
            Direction::HostToDevice => {
                self.change_state(State::ReceivingDataFromHost);
            },
            Direction::DeviceToHost => {
                self.change_state(State::SendingDataToHost);
            },
        }

        // Store the cbw
        self.command_block_wrapper = cbw;
    }

    pub fn get_current_command(&self) -> Option<&CommandBlockWrapper> {
        match self.state {
            State::SendingDataToHost |
            State::ReceivingDataFromHost => Some(&self.command_block_wrapper),
            _ => None,
        }
    }

    pub fn data_residue(&self) -> Option<u32> {
        match self.state {
            State::SendingDataToHost |
            State::ReceivingDataFromHost => Some(self.command_status_wrapper.data_residue),
            _ => None,
        }   
    }

    pub fn transfer_state(&self) -> TransferState {
        trace_bot_buffer!("BUFFER> i: {}, di: {}", self.buffer_i, self.data_i);
        match self.state {
            State::ReceivingDataFromHost => TransferState::ReceivingDataFromHost { 
                bytes_available: self.buffer_i - self.data_i,
                full: self.buffer_i == self.buffer.len(),
                done: self.command_status_wrapper.data_residue == 0,
            },
            State::SendingDataToHost => TransferState::SendingDataToHost { 
                bytes_remaining: self.buffer_i - self.data_i,
                empty: self.buffer_i == 0,
            },
            _ => TransferState::NotTransferring {
                bytes_remaining: self.buffer_i - self.data_i,
                empty: self.buffer_i == 0,
            },
        }
    }

    /// Gets a mutable slice of the buffer of the specified length
    /// panics if len requested is > the max size of the buffer
    /// returns WouldBlock if there isn't currently space in the buffer
    /// Advances the buffer pointer so don't call it unless you actually
    /// need to put data in the buffer.
    pub fn take_buffer_space(&mut self, len: usize) -> Result<&mut [u8], Error> {
        if len > self.buffer.len() {
            panic!("BulkOnlyTransport::take_buffer_space called with len > buffer.len() ({} > {}) which can never be successful",
                len, self.buffer.len());
        } 

        if len <= self.buffer.len() - self.buffer_i {
            trace_bot_buffer!("BUFFER> successfully allocated {} bytes", len);
            let s = self.buffer_i;
            let e = s + len;

            self.buffer_i += len;

            Ok(&mut self.buffer[s..e])
        } else {
            trace_bot_buffer!("BUFFER> insufficient space to allocate {} bytes", len);
            Err(WouldBlock)?
        }
    }

    /// Returns a slice containing data from the buffer if there is `len` bytes available
    /// panics if len requested is > the max size of the buffer
    /// `take_available` modifies behaviour to return whatever is available, even if that is [u8; 0]
    /// returns WouldBlock if there isn't enough data available
    pub fn take_buffered_data(&mut self, len: usize, take_available: bool) -> Result<&[u8], Error> {
        if len > self.buffer.len() {
            panic!("BulkOnlyTransport::take_buffered_data called with len > buffer.len() ({} > {}) which can never be successful",
                len, self.buffer.len());
        }

        let available = self.buffer_i - self.data_i;
        if !take_available && len > available {
            trace_bot_buffer!("BUFFER> contains insufficient data for take; requested: {}, available: {}", len, self.buffer_i - self.data_i);
            Err(WouldBlock)?
        }

        let s = self.data_i;
        let len = len.min(available);
        let e = s + len;

        self.data_i += len;
        if self.data_i == self.buffer_i {
            self.data_i = 0;
            self.buffer_i = 0;
        }
        trace_bot_buffer!("BUFFER> took {}, available after: {}", len, self.buffer_i - self.data_i);

        Ok(&self.buffer[s..e])
    }

    fn flush(&mut self) -> Result<(), Error> {
        let packet_size = self.max_packet_size() as usize;
        let residue = self.command_status_wrapper.data_residue as usize;

        let bytes = if self.data_i < self.buffer_i && residue > 0 {
            let start = self.data_i;

            // TODO: Is this the right place to enforce data_residue?
            //       It's certainly a USB feature not a SCSI/etc feature...
            let len = (self.buffer_i - self.data_i)
                .min(residue)
                .min(packet_size);

            let end = start + len;

            let bytes = self.inner.write_packet(&self.buffer[start..end])?;

            self.last_packet_full = bytes == packet_size;
            self.data_i += bytes;

            let residue = residue - bytes;
            self.command_status_wrapper.data_residue = residue as u32;

            // If we've sent all the bytes, reset the buffer indexes to 0 to free
            // up the whole buffer for more data
            if self.data_i == self.buffer_i || residue == 0{
                self.data_i = 0;
                self.buffer_i = 0;
            }

            bytes
        } else {
            0
        };

        trace_bot_bytes!("BYTES> Sent {} bytes. Data residue {} -> {}. Buff bytes: {}", 
            bytes, 
            residue, 
            self.command_status_wrapper.data_residue,
            self.buffer_i - self.data_i,
        );

        Ok(())
    }

    fn send_zlp(&mut self) -> Result<(), Error> {
        match self.inner.write_packet(&[]) {
            Ok(_) => trace_bot_zlp!("ZLP> sent"),
            Err(e) => {
                trace_bot_zlp!("ZLP> sending failed: {:?}", e);
                Err(e)?
            },
        }        

        self.change_state(State::NeedToSendStatus);

        Ok(())
    }

    fn pack_csw(&mut self) {
        self.command_status_wrapper.pack(&mut self.buffer[..CommandStatusWrapper::BYTES]).unwrap();
        self.buffer_i = CommandStatusWrapper::BYTES;
        self.data_i = 0;
        self.command_status_wrapper.data_residue = self.buffer_i as u32;
        trace_bot_headers!("HEADER> CommandStatusWrapper buffered to send: {:X?}", self.command_status_wrapper);
    }

    fn end_data_transfer(&mut self) -> Result<(), Error> {
        // Get the csw ready to send
        self.pack_csw();

        // We only send a zero length packet if the last write was a full packet AND we are sending
        // less total bytes than the command header asked for
        let needs_zlp = self.last_packet_full && 
                        self.state == State::SendingDataToHost &&
                        self.command_status_wrapper.data_residue > 0;


        // send_zlp or flush are called here because we may not get an interrupt in a timley manner
        // if we don't send immediately and
        if needs_zlp {            
            trace_bot_zlp!("ZLP> required");
            self.change_state(State::NeedZlp);
            self.send_zlp()?;
        } else {
            trace_bot_zlp!("ZLP> not required");
            self.change_state(State::NeedToSendStatus);
            self.flush()?;
        }

        Ok(())
    }

    pub fn send_command_ok(&mut self) -> Result<(), Error> {
        self.command_status_wrapper.status = CommandStatus::CommandOk;
        self.data_done = true;
        self.check_end_data_transfer()
    }

    pub fn send_command_error(&mut self) -> Result<(), Error> {
        self.command_status_wrapper.status = CommandStatus::CommandError;
        self.data_done = true;
        self.check_end_data_transfer()
    }

    fn sending_data_to_host(&mut self) -> Result<(), Error> {
        // Send as much data as possible from the current buffer
        self.flush()?;

        self.check_end_data_transfer()
    }

    fn check_end_data_transfer(&mut self) -> Result<(), Error> {
        match self.state {
            State::ReceivingDataFromHost => {
                // Check if we've read everything we were expecting
                if self.command_status_wrapper.data_residue == 0 &&
                    // AND it's been handled
                    self.data_i == self.buffer_i
                {
                    trace_bot_states!("STATE> Data residue = 0 and buffer empty, all data received");
                    self.end_data_transfer()?;
                }
            },
            State::SendingDataToHost => {
                if self.command_status_wrapper.data_residue == 0 {
                    trace_bot_states!("STATE> Data residue = 0, all data sent");
                    self.end_data_transfer()?;
                } else if self.data_done && self.data_i == self.buffer_i {
                    trace_bot_states!("STATE> Data residue > 0, early termination");
                    self.end_data_transfer()?;
                }
            }
            _ => {},
        }
        Ok(())
    }

    fn receiving_data_from_host(&mut self) -> Result<(), Error> {
        if self.command_status_wrapper.data_residue > 0 &&
            self.buffer.len() - self.buffer_i >= self.max_packet_usize() 
        {
            let bytes = self.inner.read_packet(&mut self.buffer[self.buffer_i..])?;
            self.buffer_i += bytes;

            let bytes = bytes as u32;
            let residue = self.command_status_wrapper.data_residue;
            if self.command_status_wrapper.data_residue >= bytes {
                self.command_status_wrapper.data_residue -= bytes;
            } else {
                warn!("Read more bytes that CBW offered");
                self.command_status_wrapper.data_residue = 0;
            }
            
            trace_bot_bytes!("BYTES> Read {} bytes. Data residue {} -> {}. Buff bytes: {}", 
                bytes, 
                residue, 
                self.command_status_wrapper.data_residue,
                self.buffer_i - self.data_i,
            );
        
        }

        self.check_end_data_transfer()?;

        Ok(())
    }

    fn need_to_send_status(&mut self) -> Result<(), Error> {
        self.flush()?;

        // Check if we've sent what we were asked to
        if self.command_status_wrapper.data_residue == 0 {
            self.change_state(State::WaitingForCommand);
        }

        Ok(())
    }
}

impl<B: UsbBus> UsbClass<B> for BulkOnlyTransport<'_, B> {
    fn get_configuration_descriptors(&self, writer: &mut DescriptorWriter) -> UsbResult<()> {
        self.inner.get_configuration_descriptors(writer)
    }

    fn reset(&mut self) { 
        trace_usb_control!("USB_CONTROL> reset");
        self.buffer_i = 0;
        self.data_i = 0;
        self.data_done = false;
        self.change_state(State::WaitingForCommand);
        self.inner.reset()
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = xfer.request();
        
        // Check it's the right interface first
        if !self.inner.correct_interface_number(req.index) {
            // Inner might still want to deal with it
            self.inner.control_in(xfer);
            // But we don't
            return;
        }

        let handled_res = match req {
            // Get max lun
            Request { request_type: RequestType::Class, request: REQ_GET_MAX_LUN, .. } =>
                Some(xfer.accept(|data| {
                    trace_usb_control!("USB_CONTROL> Get max lun. Response: {}", self.max_lun);
                    data[0] = self.max_lun;
                    Ok(1)
                })),

            // Bulk only mass storage reset
            Request { request_type: RequestType::Class, request: REQ_BULK_ONLY_RESET, .. } => {
                // There's some more functionality around this request to allow the reset to take
                // more time - NAK the status until the reset is done.
                // This isn't implemented.
                // See Section 3.1 [USB Bulk Only Transport Spec](https://www.usb.org/document-library/mass-storage-bulk-only-10)
                self.reset();
                Some(xfer.accept(|_| {
                    trace_usb_control!("USB_CONTROL> Bulk only mass storage reset");
                    Ok(0)
                }))
            },
            _ => {
                self.inner.control_in(xfer);
                None
            },
        };

        if let Some(Err(e)) = handled_res {
            error!("Error from ControlIn.accept: {:?}", e);
        }
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        self.inner.control_out(xfer)
    }

    fn poll(&mut self) { 
        panic!("BulkOnlyTransport::poll should never be called. Consumers (SCSI for example) should use BulkOnlyTransport::read and BulkOnlyTransport::write");
    }
}