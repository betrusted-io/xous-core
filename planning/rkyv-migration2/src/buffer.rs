use core::mem::MaybeUninit;
use std::marker::PhantomData;

use rkyv::{
    api::low::LowSerializer,
    rancor::{Panic, Strategy},
    ser::{allocator::SubAllocator, writer::Buffer as RkyvBuffer, Positional},
    with::{ArchiveWith, Identity, SerializeWith},
    Archive, Deserialize, Place, Portable, Serialize,
};
use xous::{Error, MemoryAddress, MemoryMessage, MemoryRange, MemorySize, Message, Result, CID};

use crate::testcases::{DrawStyle, PixelColor, Point, Rectangle};

const PAGE_SIZE: usize = 4096;
const PAGE_POOL_SIZE: usize = 8;
// Create a "fake" memory pool of pages, just for testing
#[repr(C, align(4096))]
pub struct Pool {
    pub bytes: [u8; PAGE_SIZE * PAGE_POOL_SIZE],
}

// The new Xous-IPC Buffer format
#[derive(Debug)]
pub struct Buffer<'buf> {
    pages: MemoryRange,
    used: usize,
    slice: &'buf mut [u8],
    should_drop: bool,
    memory_message: Option<&'buf mut MemoryMessage>,
}

// fake some allocateable memory, simply bump-allocated
static mut PAGE_POOL: Pool = Pool { bytes: [0u8; PAGE_SIZE * PAGE_POOL_SIZE] };
static mut PAGE_PTR: usize = 0;

pub fn map_memory(len: usize) -> MemoryRange {
    assert!(len % 4096 == 0);
    let address = unsafe { &PAGE_POOL.bytes[PAGE_PTR..PAGE_PTR + len] };

    let mr = unsafe { MemoryRange::new(address.as_ptr() as usize, len).unwrap() };
    unsafe { PAGE_PTR += len };
    mr
}

pub fn unmap_memory(_pages: MemoryRange) -> core::result::Result<(), xous::Error> {
    // dummy function for testing
    Ok(())
}

pub fn send_message(connection: CID, mut message: Message) -> core::result::Result<Result, Error> {
    let body = message.memory_message_mut().expect("test routine only handles mutable memory message types");

    let mut msg = xous::MessageEnvelope {
        sender: xous::sender::Sender::from_usize(1), // fake
        body: xous::Message::MutableBorrow(xous::MemoryMessage {
            id: body.id,
            buf: body.buf,
            offset: body.offset,
            valid: body.valid,
        }),
    };
    // don't actually send anything, just do the unwrap to receive in here and print the values
    match connection {
        1 => {
            // emulated TextView
            let mut buffer =
                unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
            let mut tv = buffer.to_original::<crate::testcases::TextView, _, rkyv::rancor::Error>().unwrap();

            println!("MSG: TextView");
            println!("Contents: {:?}", tv);
            tv.bounds_computed = Some(Rectangle {
                tl: Point { x: 42, y: 42 },
                br: Point { x: 420, y: 69 },
                style: DrawStyle { fill_color: None, stroke_color: Some(PixelColor::Dark), stroke_width: 3 },
            });
            buffer.replace::<Identity, _>(tv).unwrap();
            let ret = Result::MemoryReturned(body.offset, body.valid);
            println!("returning with {:?}", ret);
            Ok(ret)
        }
        _ => Err(Error::InternalError),
    }
}

type Serializer<'a, 'b> = LowSerializer<'a, RkyvBuffer<'b>, SubAllocator<'a>, Panic>;

impl<'buf> Buffer<'buf> {
    pub fn new(len: usize) -> Self {
        let len_to_page = (len + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);

        // Allocate enough memory to hold the requested data
        let new_mem = map_memory(len_to_page);

        Buffer {
            pages: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), len_to_page) },
            used: 0,
            should_drop: true,
            memory_message: None,
        }
    }

    pub fn into_buf<F, T>(src: &T) -> Self
    where
        F: for<'a, 'b> SerializeWith<T, Serializer<'a, 'b>>,
    {
        struct Wrap<'a, F, T>(&'a T, PhantomData<F>);

        impl<F, T> Archive for Wrap<'_, F, T>
        where
            F: ArchiveWith<T>,
        {
            type Archived = <F as ArchiveWith<T>>::Archived;
            type Resolver = <F as ArchiveWith<T>>::Resolver;

            fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
                F::resolve_with(self.0, resolver, out)
            }
        }

        impl<'a, 'b, F, T> Serialize<Serializer<'a, 'b>> for Wrap<'_, F, T>
        where
            F: SerializeWith<T, Serializer<'a, 'b>>,
        {
            fn serialize(
                &self,
                serializer: &mut Serializer<'a, 'b>,
            ) -> core::result::Result<Self::Resolver, Panic> {
                F::serialize_with(self.0, serializer)
            }
        }
        let mut xous_buf = Self::new(core::mem::size_of::<T>());
        let mut scratch = [MaybeUninit::<u8>::uninit(); 256];

        let wrap = Wrap(src, PhantomData::<F>);
        let writer = RkyvBuffer::from(&mut xous_buf.slice[..]);
        let alloc = SubAllocator::new(&mut scratch);

        let serbuf = rkyv::api::low::to_bytes_in_with_alloc::<_, _, Panic>(&wrap, writer, alloc).unwrap();
        xous_buf.used = serbuf.pos();
        println!("pos: {}", xous_buf.used);
        println!("scratch: {:x?}", &scratch[..16]);
        xous_buf
    }

    #[allow(dead_code)]
    pub fn replace<F, T>(&mut self, src: T) -> core::result::Result<(), &'static str>
    where
        F: for<'a, 'b> SerializeWith<T, Serializer<'a, 'b>>,
    {
        struct Wrap<'a, F, T>(&'a T, PhantomData<F>);

        impl<F, T> Archive for Wrap<'_, F, T>
        where
            F: ArchiveWith<T>,
        {
            type Archived = <F as ArchiveWith<T>>::Archived;
            type Resolver = <F as ArchiveWith<T>>::Resolver;

            fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
                F::resolve_with(self.0, resolver, out)
            }
        }

        impl<'a, 'b, F, T> Serialize<Serializer<'a, 'b>> for Wrap<'_, F, T>
        where
            F: SerializeWith<T, Serializer<'a, 'b>>,
        {
            fn serialize(
                &self,
                serializer: &mut Serializer<'a, 'b>,
            ) -> core::result::Result<Self::Resolver, Panic> {
                F::serialize_with(self.0, serializer)
            }
        }

        // We must have a `memory_message` to update in order for this to work.
        // Otherwise, we risk having the pointer go to somewhere invalid.
        if self.memory_message.is_none() {
            // Create this message using `from_memory_message_mut()` instead of
            // `from_memory_message()`.
            Err("couldn't serialize because buffer wasn't mutable")?;
        }
        // Unsafe Warning: Create a copy of the backing slice to hand to the deserializer.
        // This is required because the deserializer consumes the buffer and returns it
        // later as part of `.into_inner()`.
        // The "correct" way to do this would be to implement `rkyv::Serializer` an `rkyv::Fallible`
        // for ourselves.
        let copied_slice =
            unsafe { core::slice::from_raw_parts_mut(self.slice.as_mut_ptr(), self.slice.len()) };
        let mut scratch = [MaybeUninit::<u8>::uninit(); 256];

        let wrap = Wrap(&src, PhantomData::<F>);
        let writer = RkyvBuffer::from(&mut copied_slice[..]);
        let alloc = SubAllocator::new(&mut scratch);

        let serbuf = rkyv::api::low::to_bytes_in_with_alloc::<_, _, Panic>(&wrap, writer, alloc).unwrap();
        self.used = serbuf.pos();

        if let Some(ref mut msg) = self.memory_message.as_mut() {
            msg.offset = MemoryAddress::new(self.used);
        }
        Ok(())
    }

    pub fn to_original<T, U, E>(&self) -> core::result::Result<T, E>
    where
        T: rkyv::Archive<Archived = U>,
        U: Portable,
        E: std::fmt::Debug,
        <T as Archive>::Archived: Deserialize<T, Strategy<rkyv::de::Pool, E>>,
    {
        let r = unsafe { rkyv::access_unchecked::<U>(&self.slice[..self.used]) };
        Ok(rkyv::deserialize::<T, E>(r).unwrap())
    }

    pub fn as_flat<T, U>(&self) -> core::result::Result<&U, ()>
    where
        T: rkyv::Archive<Archived = U>,
        U: Portable,
    {
        let r = unsafe { rkyv::access_unchecked::<U>(&self.slice[..self.used]) };
        Ok(r)
    }

    pub fn used(&self) -> usize { self.used }

    /// Perform a mutable lend of this Buffer to the server.
    pub fn lend_mut(&mut self, connection: CID, id: u32) -> core::result::Result<Result, Error> {
        let msg = MemoryMessage {
            id: id as usize,
            buf: self.pages,
            offset: MemoryAddress::new(self.used),
            valid: MemorySize::new(self.pages.len()),
        };

        // Update the offset pointer if the server modified it.
        println!("Just before send_message");
        let result = send_message(connection, Message::MutableBorrow(msg));
        println!("result: {:?}", result);
        if let Ok(Result::MemoryReturned(offset, _valid)) = result {
            self.used = offset.map_or(0, |v| v.get());
        }

        result
    }

    // use to serialize a buffer between process-local threads. mainly for spawning new threads with more
    // complex argument structures.
    #[allow(dead_code)]
    pub unsafe fn to_raw_parts(&self) -> (usize, usize, usize) {
        (self.pages.as_ptr() as usize, self.pages.len(), self.used)
    }

    // use to serialize a buffer between process-local threads. mainly for spawning new threads with more
    // complex argument structures.
    #[allow(dead_code)]
    pub unsafe fn from_raw_parts(address: usize, len: usize, offset: usize) -> Self {
        let mem = MemoryRange::new(address, len).expect("invalid memory range args");
        Buffer {
            pages: mem,
            slice: core::slice::from_raw_parts_mut(mem.as_mut_ptr(), mem.len()),
            used: offset,
            should_drop: false,
            memory_message: None,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message(mem: &'buf MemoryMessage) -> Self {
        Buffer {
            pages: mem.buf,
            slice: core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len()),
            used: mem.offset.map_or(0, |v| v.get()),
            should_drop: false,
            memory_message: None,
        }
    }

    #[allow(dead_code)]
    pub unsafe fn from_memory_message_mut(mem: &'buf mut MemoryMessage) -> Self {
        Buffer {
            pages: mem.buf,
            slice: core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr(), mem.buf.len()),
            used: mem.offset.map_or(0, |v| v.get()),
            should_drop: false,
            memory_message: Some(mem),
        }
    }
}

impl<'a> core::convert::AsRef<[u8]> for Buffer<'a> {
    fn as_ref(&self) -> &[u8] { &self.slice[..self.used] }
}

impl<'a> core::convert::AsMut<[u8]> for Buffer<'a> {
    fn as_mut(&mut self) -> &mut [u8] { &mut self.slice[..self.used] }
}

impl<'a> core::ops::Deref for Buffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target { &*(&self.slice[..self.used]) }
}

impl<'a> core::ops::DerefMut for Buffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *(&mut self.slice[..self.used]) }
}

impl<'a> Drop for Buffer<'a> {
    fn drop(&mut self) {
        if self.should_drop {
            unmap_memory(self.pages).expect("Buffer: failed to drop memory");
        }
    }
}
