use core::mem::MaybeUninit;

use rkyv::{
    api::low::LowSerializer, deserialize, rancor::{Error, Panic, Strategy}, ser::{allocator::SubAllocator,
        writer::Buffer as RkyvBuffer,
        Positional,
    }, with::{ArchiveWith, Identity, SerializeWith}, Archive, Deserialize, Place, Portable, Serialize
};

use std::marker::PhantomData;

use xous::{MemoryMessage, MemoryRange};

const PAGE_SIZE: usize = 4096;
const PAGE_POOL_SIZE: usize = 8;
// Create a "fake" memory pool of pages, just for testing
#[repr(C, align(4096))]
pub struct Pool {
    pub bytes: [u8; PAGE_SIZE * PAGE_POOL_SIZE]
}

// A test structure with rkyv derives
#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[rkyv(
    // This will generate a PartialEq impl between our unarchived
    // and archived types
    compare(PartialEq),
    // Derives can be passed through to the generated type:
    derive(Debug),
)]
struct Test {
    int: u8,
    string: String,
    option: Option<Vec<i32>>,
    string2: String,
}


// The new Xous-IPC Buffer format
#[derive(Debug)]
pub struct Buffer<'buf> {
    pages: MemoryRange,
    alloc_at: usize,
    pos: usize,
    slice: &'buf mut [u8],
    should_drop: bool,
    memory_message: Option<&'buf mut MemoryMessage>,
}
// what is a safe number for this? empirically, 20 bytes seems to be the overhead we've measured
const RKYV_OVERHEAD: usize = 32;

// fake some allocateable memory, simply bump-allocated
static mut PAGE_POOL: Pool = Pool {bytes: [0u8; PAGE_SIZE * PAGE_POOL_SIZE]};
static mut PAGE_PTR: usize = 0;

pub fn map_memory(len: usize) -> MemoryRange {
    assert!(len % 4096 == 0);
    let address = unsafe{& PAGE_POOL.bytes[PAGE_PTR..PAGE_PTR + len]};

    let mr = unsafe{MemoryRange::new(address.as_ptr() as usize, len).unwrap()};
    unsafe{ PAGE_PTR += len };
    mr
}

type Serializer<'a, 'b> =
    LowSerializer<'a, RkyvBuffer<'b>, SubAllocator<'a>, Panic>;

impl<'buf> Buffer<'buf> {
    pub fn new(len: usize) -> Self {
        let len_to_page = (len + (PAGE_SIZE -1)) & !(PAGE_SIZE - 1);
        // let alloc_start = len + RKYV_OVERHEAD;
        let alloc_start = len_to_page - RKYV_OVERHEAD;

        // Allocate enough memory to hold the requested data
        let new_mem = map_memory(len_to_page);

        Buffer {
            pages: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), len_to_page) },
            pos: 0,
            alloc_at: alloc_start,
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

            fn resolve(
                &self,
                resolver: Self::Resolver,
                out: Place<Self::Archived>,
            ) {
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
            ) -> Result<Self::Resolver, Panic> {
                F::serialize_with(self.0, serializer)
            }
        }
        let mut xous_buf = Self::new(core::mem::size_of::<T>());
        println!("alloc at: {}", xous_buf.alloc_at);
        let (buf, scratch) = xous_buf.slice.split_at_mut(xous_buf.alloc_at);

        let wrap = Wrap(src, PhantomData::<F>);
        let writer = RkyvBuffer::from(buf);
        let maybe_uninit_slice: &mut [MaybeUninit<u8>] = unsafe {
            std::slice::from_raw_parts_mut(scratch.as_ptr() as *mut MaybeUninit<u8>, scratch.len())
        };
        let alloc = SubAllocator::new(maybe_uninit_slice);

        let serbuf = rkyv::api::low::to_bytes_in_with_alloc::<_, _, Panic>(&wrap, writer, alloc).unwrap();
        xous_buf.pos = serbuf.pos();
        println!("pos: {}", xous_buf.pos);
        println!("scratch: {:x?}", &scratch[..32]);
        xous_buf
    }

    pub fn to_original<T, U, E>(&self) -> core::result::Result<T, E>
    where
        T: rkyv::Archive<Archived = U>,
        U: Portable,
        E: std::fmt::Debug, <T as Archive>::Archived: Deserialize<T, Strategy<rkyv::de::Pool, E>>
    {
        let r = unsafe{rkyv::access_unchecked::<U>(&self.slice[..self.pos])};
        Ok(rkyv::deserialize::<T, E>(r).unwrap())
    }

    pub fn as_flat<T, U>(&self) -> core::result::Result<&U, ()>
    where
        T: rkyv::Archive<Archived = U>,
        U: Portable
    {
        let r = unsafe{rkyv::access_unchecked::<U>(&self.slice[..self.pos])};
        Ok(r)
    }
}


fn main() {
    println!("Size of Test: {}", core::mem::size_of::<Test>());

    // with an alloc
    let value = Test {
        int: 42,
        string: "The quick brown fox jumps over the lazy dogs".to_string(),
        option: Some(vec![1, 2, 3, 4]),
        string2: "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.".to_string(),
    };

    let buf = Buffer::into_buf::<Identity, Test>(&value); // AsBox

    let f = buf.as_flat::<Test, _>().unwrap();
    println!("f: {:?}", f);

    let t1 = buf.to_original::<Test, _, Error>().unwrap();
    println!("t copy 1: {:?}", t1);

    println!("buf.slice: {:x?}", &buf.slice[..buf.pos+1]);
    let archived = unsafe{rkyv::access_unchecked::<ArchivedTest>(&buf.slice[..buf.pos])};
    println!("archived: {:?}", archived);
    let t = rkyv::deserialize::<Test, Error>(archived).unwrap();
    println!("t: {:?}", t);
    assert_eq!(t.string, value.string);

    use rkyv::ser::allocator::Arena;
    use rkyv::api::high::to_bytes_with_alloc;
    let mut arena = Arena::new();

    // let de = buf.to_original::<Test, rkyv::rancor::Error>().unwrap();
    let bytes =
        to_bytes_with_alloc::<_, Error>(&value, arena.acquire()).unwrap();
    let archived =
        unsafe { rkyv::access_unchecked::<ArchivedTest>(&bytes[..]) };
    assert_eq!(archived, &value);
    let deserialized = deserialize::<Test, Error>(archived).unwrap();
    assert_eq!(deserialized.string2, value.string2);
}
