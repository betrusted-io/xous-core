use xous::MessageEnvelope;

pub struct FlashDrive {
    capacity: usize,
    block_size: usize,
    memory: xous::MemoryRange,
}

#[derive(Debug)]
pub enum FlashDriveError {
    CapacityNotPageAligned,
}

impl FlashDrive {
    pub fn new(capacity: usize, block_size: usize) -> Result<Self, FlashDriveError> {
        if (capacity % 1024) != 0 {
            return Err(FlashDriveError::CapacityNotPageAligned);
        }

        let mut backing =
            xous::syscall::map_memory(None, None, capacity, xous::MemoryFlags::R | xous::MemoryFlags::W)
                .unwrap();

        // initialize backing slice with bogus data. Safety: all `[u32]`
        // values are valid
        let backing_slice: &mut [u32] = unsafe { backing.as_slice_mut() };
        for (index, d) in backing_slice.iter_mut().enumerate() {
            *d = index as u32;
        }

        Ok(Self { capacity, block_size, memory: backing })
    }

    pub fn read(&mut self, msg: &mut MessageEnvelope) {
        let body = msg.body.memory_message_mut().expect("incorrect message type received");
        let lba = body.offset.map(|v| v.get()).unwrap_or_default();
        // Safety: all values of `[u32]` are valid
        let data = unsafe { body.buf.as_slice_mut::<u8>() };

        self.read_inner(lba, data);
    }

    pub fn write(&mut self, msg: &mut MessageEnvelope) {
        let body = msg.body.memory_message_mut().expect("incorrect message type received");
        let lba = body.offset.map(|v| v.get()).unwrap_or_default();
        // Safety: all values of `[u32]` are valid
        let data = unsafe { body.buf.as_slice_mut::<u8>() };

        self.write_inner(lba, data);
    }

    pub fn max_lba(&self, msg: &mut MessageEnvelope) {
        xous::return_scalar(msg.sender, self.max_lba_inner() as usize).unwrap();
    }
}

impl FlashDrive {
    fn read_inner(&mut self, lba: usize, data: &mut [u8]) {
        // Safety: all values of `[u8]` are valid
        let backing_slice: &[u8] = unsafe { self.memory.as_slice() };

        let rawdata = &backing_slice[lba * self.block_size..(lba + 1) * self.block_size];

        data[..self.block_size].copy_from_slice(rawdata);
    }

    fn write_inner(&mut self, lba: usize, data: &mut [u8]) {
        // Safety: all values of `[u8]` are valid
        let backing_slice: &mut [u8] = unsafe { self.memory.as_slice_mut() };
        backing_slice[lba * self.block_size..(lba + 1) * self.block_size]
            .copy_from_slice(&data[..self.block_size]);
    }

    fn max_lba_inner(&self) -> u32 { (self.capacity as u32 / 512) - 1 }
}
