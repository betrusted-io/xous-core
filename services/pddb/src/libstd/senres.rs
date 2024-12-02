#![allow(unused)]
use core::cell::Cell;
use core::convert::TryInto;

/// Senres V1 always begins with the number 0x344cb6ca to indicate it's valid.
/// This number will change on subsequent versions.
const SENRES_V1_MAGIC: u32 = 0x344cb6ca;

#[cfg(target_os = "xous")]
/// Copies of these invocation types here for when we're running
/// in environments without libxous.
pub enum InvokeType {
    LendMut = 1,
    Lend = 2,
    Move = 3,
    Scalar = 4,
    BlockingScalar = 5,
}

#[cfg(target_os = "xous")]
/// Copies of these invocation types here for when we're running
/// in environments without libxous.
pub enum Syscall {
    SendMessage = 16,
    ReturnMemory = 20,
}

#[cfg(target_os = "xous")]
/// Copies of these invocation types here for when we're running
/// in environments without libxous.
pub enum SyscallResult {
    Scalar1 = 14,
    Scalar2 = 15,
    MemoryReturned = 18,
}

/// A struct to send and receive data. This struct must be page-aligned
/// in order to be sendable across processes.
#[repr(C, align(4096))]
pub struct Stack<const N: usize = 4096> {
    data: [u8; N],
}

/// A version of the message on the receiving side, reconstituted from
/// a slice from a message.
pub struct Message<'a> {
    message_id: usize,
    auto_return: bool,
    data: &'a [u8],
}

/// A version of the message on the receiving side, reconstituted from
/// a slice from a message.
pub struct MutableMessage<'a> {
    message_id: usize,
    mutable: bool,
    auto_return: bool,
    data: &'a mut [u8],
}

impl<'a> Message<'a> {
    pub fn new(message_id: usize, data: usize, len: usize, mutable: bool) -> Result<Self, ()> {
        if data & 4095 != 0 || len & 4095 != 0 {
            return Err(());
        }
        Ok(Message {
            message_id,
            auto_return: true,
            data: unsafe { core::slice::from_raw_parts_mut(data as *mut u8, len) },
        })
    }

    // Need to figure out how to make the non-mutable version work, since `data`
    // must be a mutable vec.
    pub fn from_slice(data: &'a [u8]) -> Result<Self, ()> {
        Ok(Message { message_id: 0, auto_return: false, data })
    }

    pub fn from_mut_slice(data: &'a mut [u8]) -> Result<MutableMessage<'a>, ()> {
        Ok(MutableMessage { message_id: 0, mutable: true, auto_return: false, data })
    }

    pub fn auto_return(&mut self, enable: bool) { self.auto_return = enable; }
}

#[cfg(target_os = "xous")]
fn return_memory_message(message_id: usize, data_addr: usize, data_len: usize) -> usize {
    let a0 = Syscall::ReturnMemory as usize;

    // "Offset"
    let a4 = 0;

    // "Valid"
    let a5 = 0;

    let mut result: usize;

    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0 => result,
            inlateout("a1") message_id => _,
            inlateout("a2") data_addr => _,
            inlateout("a3") data_len=> _,
            inlateout("a4") a4 => _,
            inlateout("a5") a5 => _,
            out("a6") _,
            out("a7") _,
        )
    };
    result
}

impl<'a> Drop for Message<'a> {
    fn drop(&mut self) {
        if self.auto_return {
            #[cfg(target_os = "xous")]
            return_memory_message(self.message_id, self.data.as_ptr() as usize, self.data.len());
        }
    }
}

impl<'a> SenresMut for MutableMessage<'a> {
    fn as_mut_slice(&mut self) -> &mut [u8] { self.data }

    fn as_mut_ptr(&mut self) -> *mut u8 { self as *mut _ as *mut u8 }
}

impl<'a> Senres for MutableMessage<'a> {
    fn as_slice(&self) -> &[u8] { self.data }

    fn len(&self) -> usize { self.data.len() }

    fn as_ptr(&self) -> *const u8 { self as *const _ as *const u8 }

    fn can_create_writer(&self) -> bool { self.mutable }
}

impl<'a> Senres for Message<'a> {
    fn as_slice(&self) -> &[u8] { self.data }

    fn len(&self) -> usize { self.data.len() }

    fn as_ptr(&self) -> *const u8 { self as *const _ as *const u8 }

    fn can_create_writer(&self) -> bool { false }
}

pub trait Senres {
    fn as_slice(&self) -> &[u8];
    fn as_ptr(&self) -> *const u8;
    fn len(&self) -> usize;

    fn can_create_writer(&self) -> bool { true }

    fn reader(&self, fourcc: [u8; 4]) -> Option<Reader<Self>>
    where
        Self: core::marker::Sized,
    {
        let reader = Reader { backing: self, offset: core::cell::Cell::new(0) };
        if SENRES_V1_MAGIC != reader.try_get_from::<u32>().ok()? {
            return None;
        }
        let target_fourcc: [u8; 4] = reader.try_get_from().ok()?;
        if target_fourcc != fourcc {
            return None;
        }
        Some(reader)
    }

    #[cfg(not(target_os = "xous"))]
    fn lend(&self, _connection: u32, _opcode: usize) -> Result<(), ()> { Ok(()) }

    #[cfg(target_os = "xous")]
    fn lend(&self, connection: u32, opcode: usize) -> Result<(), ()> {
        let mut a0 = Syscall::SendMessage as usize;
        let a1: usize = connection.try_into().unwrap();
        let a2 = InvokeType::Lend as usize;
        let a3 = opcode;
        let a4 = self.as_ptr() as usize;
        let a5 = self.len();

        unsafe {
            core::arch::asm!(
                "ecall",
                inlateout("a0") a0,
                inlateout("a1") a1 => _,
                inlateout("a2") a2 => _,
                inlateout("a3") a3 => _,
                inlateout("a4") a4 => _,
                inlateout("a5") a5 => _,
                out("a6") _,
                out("a7") _,
            )
        };

        let result = a0;
        if result == SyscallResult::MemoryReturned as usize {
            Ok(())
        } else {
            println!("Unexpected memory return value: {}", result);
            Err(())
        }
    }
}

pub trait SenresMut: Senres {
    fn as_mut_slice(&mut self) -> &mut [u8];
    fn as_mut_ptr(&mut self) -> *mut u8;
    fn writer(&mut self, fourcc: [u8; 4]) -> Option<Writer<Self>>
    where
        Self: core::marker::Sized,
    {
        if !self.can_create_writer() {
            return None;
        }
        let mut writer = Writer { backing: self, offset: 0 };
        writer.append(SENRES_V1_MAGIC);
        writer.append(fourcc);
        Some(writer)
    }
    #[cfg(not(target_os = "xous"))]
    fn lend_mut(&mut self, _connection: u32, _opcode: usize) -> Result<(), ()> { Ok(()) }
    #[cfg(target_os = "xous")]
    fn lend_mut(&mut self, connection: u32, opcode: usize) -> Result<(), ()> {
        let mut a0 = Syscall::SendMessage as usize;
        let mut a1: usize = connection.try_into().unwrap();
        let a2 = InvokeType::LendMut as usize;
        let a3 = opcode;
        let a4 = self.as_mut_ptr() as usize;
        let a5 = self.len();

        unsafe {
            core::arch::asm!(
                "ecall",
                inlateout("a0") a0,
                inlateout("a1") a1,
                inlateout("a2") a2 => _,
                inlateout("a3") a3 => _,
                inlateout("a4") a4 => _,
                inlateout("a5") a5 => _,
                out("a6") _,
                out("a7") _,
            )
        };

        let result = a0;
        // let offset = a1;
        // let valid = a2;

        if result == SyscallResult::MemoryReturned as usize {
            Ok(())
        } else {
            println!("Unexpected memory return value: {} ({})", result, a1);
            Err(())
        }
    }
}

pub struct Writer<'a, Backing: SenresMut> {
    backing: &'a mut Backing,
    offset: usize,
}

pub struct DelayedWriter<Backing: SenresMut, T: SenSer<Backing>> {
    offset: usize,
    _kind: core::marker::PhantomData<T>,
    _backing: core::marker::PhantomData<Backing>,
}

pub struct Reader<'a, Backing: Senres> {
    backing: &'a Backing,
    offset: Cell<usize>,
}

pub trait SenSer<Backing: SenresMut> {
    fn append_to(&self, senres: &mut Writer<Backing>);
}

pub trait RecDes<Backing: Senres> {
    fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()>
    where
        Self: core::marker::Sized;
}

pub trait RecDesRef<'a, Backing: Senres> {
    fn try_get_ref_from(senres: &'a Reader<Backing>) -> Result<&'a Self, ()>;
}

impl<const N: usize> Stack<N> {
    /// Ensure that `N` is a multiple of 4096. This constant should
    /// be evaluated in the constructor function.
    const CHECK_ALIGNED: () = if N & 4095 != 0 {
        panic!("Senres size must be a multiple of 4096")
    };

    pub const fn new() -> Self {
        // Ensure the `N` that was specified is a multiple of 4096
        #[allow(clippy::no_effect, clippy::let_unit_value)]
        let _ = Self::CHECK_ALIGNED;
        Stack { data: [0u8; N] }
    }
}

impl<const N: usize> SenresMut for Stack<N> {
    fn as_mut_slice(&mut self) -> &mut [u8] { self.data.as_mut_slice() }

    fn as_mut_ptr(&mut self) -> *mut u8 { &mut self.data as *mut _ as *mut u8 }
}

impl<const N: usize> Senres for Stack<N> {
    fn as_slice(&self) -> &[u8] { self.data.as_slice() }

    fn len(&self) -> usize { N }

    fn as_ptr(&self) -> *const u8 { &self.data as *const _ as *const u8 }
}

impl<'a, Backing: SenresMut> Writer<'a, Backing> {
    pub fn append<T: SenSer<Backing>>(&mut self, other: T) { other.append_to(self); }

    pub fn delayed_append<T: SenSer<Backing>>(&mut self) -> DelayedWriter<Backing, T> {
        let delayed_writer = DelayedWriter {
            offset: self.offset,
            _backing: core::marker::PhantomData::<Backing>,
            _kind: core::marker::PhantomData::<T>,
        };
        self.offset += core::mem::size_of::<T>();
        delayed_writer
    }

    pub fn do_delayed_append<T: SenSer<Backing>>(
        &mut self,
        delayed_writer: DelayedWriter<Backing, T>,
        other: T,
    ) {
        let current_offset = self.offset;
        self.offset = delayed_writer.offset;
        other.append_to(self);
        if self.offset != delayed_writer.offset + core::mem::size_of::<T>() {
            panic!("writer incorrectly increased offset");
        }
        self.offset = current_offset;
    }

    pub fn align_to(&mut self, alignment: usize) {
        while self.offset & (alignment - 1) != 0 {
            self.offset += 1;
        }
    }
}

impl<'a, Backing: Senres> Reader<'a, Backing> {
    pub fn try_get_from<T: RecDes<Backing>>(&self) -> Result<T, ()> { T::try_get_from(self) }

    pub fn try_get_ref_from<T: RecDesRef<'a, Backing> + ?Sized>(&'a self) -> Result<&'a T, ()> {
        T::try_get_ref_from(self)
    }

    fn align_to(&self, alignment: usize) {
        while self.offset.get() & (alignment - 1) != 0 {
            self.offset.set(self.offset.get() + 1);
        }
    }
}

macro_rules! primitive_impl {
    ($SelfT:ty) => {
        impl<Backing: SenresMut> SenSer<Backing> for $SelfT {
            fn append_to(&self, senres: &mut Writer<Backing>) {
                senres.align_to(core::mem::align_of::<Self>());
                for (src, dest) in
                    self.to_le_bytes().iter().zip(senres.backing.as_mut_slice()[senres.offset..].iter_mut())
                {
                    *dest = *src;
                    senres.offset += 1;
                }
            }
        }

        impl<Backing: Senres> RecDes<Backing> for $SelfT {
            fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()> {
                senres.align_to(core::mem::align_of::<Self>());
                let my_size = core::mem::size_of::<Self>();
                let offset = senres.offset.get();
                if offset + my_size > senres.backing.as_slice().len() {
                    return Err(());
                }
                let val = Self::from_le_bytes(
                    senres.backing.as_slice()[offset..offset + my_size].try_into().unwrap(),
                );
                senres.offset.set(offset + my_size);
                Ok(val)
            }
        }
    };
}

impl<Backing: SenresMut> SenSer<Backing> for bool {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        senres.align_to(core::mem::align_of::<Self>());
        senres.backing.as_mut_slice()[senres.offset] = if *self { 1 } else { 0 };
        senres.offset += 1;
    }
}

impl<Backing: Senres> RecDes<Backing> for bool {
    fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()> {
        senres.align_to(core::mem::align_of::<Self>());
        let my_size = core::mem::size_of::<Self>();
        let offset = senres.offset.get();
        if offset + my_size > senres.backing.as_slice().len() {
            return Err(());
        }
        let val = match senres.backing.as_slice()[offset] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        };
        senres.offset.set(offset + my_size);
        val
    }
}

impl<T: SenSer<Backing>, Backing: SenresMut> SenSer<Backing> for Option<T> {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        if let Some(val) = self {
            senres.append(1u8);
            val.append_to(senres);
        } else {
            senres.append(0u8);
        }
    }
}

impl<T: RecDes<Backing>, Backing: Senres> RecDes<Backing> for Option<T> {
    fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()> {
        if senres.offset.get() + 1 > senres.backing.as_slice().len() {
            return Err(());
        }
        let check = senres.try_get_from::<u8>()?;
        if check == 0 {
            return Ok(None);
        }
        if check != 1 {
            return Err(());
        }
        let my_size = core::mem::size_of::<Self>();
        if senres.offset.get() + my_size > senres.backing.as_slice().len() {
            return Err(());
        }
        Ok(Some(T::try_get_from(senres)?))
    }
}

primitive_impl! {u8}
primitive_impl! {i8}
primitive_impl! {u16}
primitive_impl! {i16}
primitive_impl! {u32}
primitive_impl! {i32}
primitive_impl! {u64}
primitive_impl! {i64}

impl<T: SenSer<Backing>, Backing: SenresMut> SenSer<Backing> for &[T] {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        senres.append(self.len() as u32);
        for entry in self.iter() {
            entry.append_to(senres)
        }
    }
}

impl<T: SenSer<Backing>, Backing: SenresMut, const N: usize> SenSer<Backing> for [T; N] {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        // senres.append(self.len() as u32);
        senres.align_to(core::mem::align_of::<Self>());
        for entry in self.iter() {
            entry.append_to(senres)
        }
    }
}

impl<T: RecDes<Backing>, Backing: Senres, const N: usize> RecDes<Backing> for [T; N] {
    fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()> {
        let len = core::mem::size_of::<Self>();
        senres.align_to(core::mem::align_of::<Self>());
        let offset = senres.offset.get();
        if offset + len > senres.backing.as_slice().len() {
            return Err(());
        }

        // See https://github.com/rust-lang/rust/issues/61956 for why this
        // is awful
        let mut output: [core::mem::MaybeUninit<T>; N] =
            unsafe { core::mem::MaybeUninit::uninit().assume_init() };
        for elem in &mut output[..] {
            elem.write(T::try_get_from(senres)?);
        }

        // Using &mut as an assertion of unique "ownership"
        let ptr = &mut output as *mut _ as *mut [T; N];
        let res = unsafe { ptr.read() };
        core::mem::forget(output);
        Ok(res)
    }
}

impl<Backing: SenresMut> SenSer<Backing> for str {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        senres.append(self.len() as u32);
        for (src, dest) in
            self.as_bytes().iter().zip(senres.backing.as_mut_slice()[senres.offset..].iter_mut())
        {
            *dest = *src;
            senres.offset += 1;
        }
    }
}

impl<Backing: SenresMut> SenSer<Backing> for &str {
    fn append_to(&self, senres: &mut Writer<Backing>) {
        senres.append(self.len() as u32);
        for (src, dest) in
            self.as_bytes().iter().zip(senres.backing.as_mut_slice()[senres.offset..].iter_mut())
        {
            *dest = *src;
            senres.offset += 1;
        }
    }
}

impl<Backing: Senres> RecDes<Backing> for String {
    fn try_get_from(senres: &Reader<Backing>) -> Result<Self, ()> {
        let len = senres.try_get_from::<u32>()? as usize;
        let offset = senres.offset.get();
        if offset + len > senres.backing.as_slice().len() {
            return Err(());
        }
        core::str::from_utf8(&senres.backing.as_slice()[offset..offset + len]).or(Err(())).map(|e| {
            senres.offset.set(offset + len);
            e.to_owned()
        })
    }
}

impl<'a, Backing: Senres> RecDesRef<'a, Backing> for str {
    fn try_get_ref_from(senres: &'a Reader<Backing>) -> Result<&'a Self, ()> {
        let len = senres.try_get_from::<u32>()? as usize;
        let offset = senres.offset.get();
        if offset + len > senres.backing.as_slice().len() {
            return Err(());
        }
        core::str::from_utf8(&senres.backing.as_slice()[offset..offset + len]).or(Err(())).map(|e| {
            senres.offset.set(offset + len);
            e
        })
    }
}

impl<'a, Backing: Senres, T: RecDes<Backing>> RecDesRef<'a, Backing> for [T] {
    fn try_get_ref_from(senres: &'a Reader<Backing>) -> Result<&'a Self, ()> {
        let len = senres.try_get_from::<u32>()? as usize;
        let offset = senres.offset.get();
        if offset + (len * core::mem::size_of::<T>()) > senres.backing.as_slice().len() {
            return Err(());
        }
        let ret = unsafe {
            core::slice::from_raw_parts(senres.backing.as_slice().as_ptr().add(offset) as *const T, len)
        };
        senres.offset.set(offset + len * core::mem::size_of::<T>());
        Ok(ret)
    }
}

impl<const N: usize> Default for Stack<N> {
    fn default() -> Self { Self::new() }
}

fn do_stuff(_r: &Stack) {
    println!("Stuff!");
}

#[test]
fn smoke_test() {
    let mut sr1 = Stack::<4096>::new();
    // let sr2 = Senres::<4097>::new();
    let sr3 = Stack::<8192>::new();
    // let sr4 = Senres::<4098>::new();
    let sr5 = Stack::new();

    do_stuff(&sr5);
    println!("Size of sr1: {}", core::mem::size_of_val(&sr1));
    println!("Size of sr3: {}", core::mem::size_of_val(&sr3));
    println!("Size of sr5: {}", core::mem::size_of_val(&sr5));

    {
        let mut writer = sr1.writer(*b"test").unwrap();
        writer.append(16777215u32);
        writer.append(u64::MAX);
        writer.append("Hello, world!");
        writer.append("String2");
        writer.append::<Option<u32>>(None);
        writer.append::<Option<u32>>(Some(42));
        writer.append(96u8);
        writer.append([1i32, 2, 3, 4, 5].as_slice());
        writer.append([5u8, 4, 3, 2].as_slice());
        writer.append([5u16, 4, 2]);
        writer.append(["Hi", "There", "123456789"]);
        // writer.append(["Hello".to_owned(), "There".to_owned(), "World".to_owned()].as_slice());
    }
    // println!("sr1: {:?}", sr1);

    {
        let reader = sr1.reader(*b"test").expect("couldn't get reader");
        let val: u32 = reader.try_get_from().expect("couldn't get the u32 value");
        println!("u32 val: {}", val);
        let val: u64 = reader.try_get_from().expect("couldn't get the u64 value");
        println!("u64 val: {:x}", val);
        let val: &str = reader.try_get_ref_from().expect("couldn't get string value");
        println!("String val: {}", val);
        let val: String = reader.try_get_from().expect("couldn't get string2 value");
        println!("String2 val: {}", val);
        let val: Option<u32> = reader.try_get_from().expect("couldn't get Option<u32>");
        println!("Option<u32> val: {:?}", val);
        let val: Option<u32> = reader.try_get_from().expect("couldn't get Option<u32>");
        println!("Option<u32> val: {:?}", val);

        let val: u8 = reader.try_get_from().expect("couldn't get u8 weird padding");
        println!("u8 val: {}", val);

        let val: &[i32] = reader.try_get_ref_from().expect("couldn't get &[i32]");
        println!("&[i32] val: {:?}", val);
        let val: &[u8] = reader.try_get_ref_from().expect("couldn't get &[u8]");
        println!("&[u8] val: {:?}", val);
        let val: [u16; 3] = reader.try_get_from().expect("couldn't get [u16; 3]");
        println!("[u16; 3] val: {:?}", val);
        let val: [String; 3] = reader.try_get_from().expect("couldn't get [String; 3]");
        println!("[String; 3] val: {:?}", val);
    }
}
