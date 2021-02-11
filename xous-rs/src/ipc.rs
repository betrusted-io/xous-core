use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::{PhantomData, Unpin};
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

#[derive(PartialEq, Debug)]
enum MemoryState {
    /// This struct is available for lending and sending
    Available,

    /// This memory has been mutably lent out and will be returned
    MutablyLent,

    /// This memory was moved, and should be forgotten
    Moved,
}

// #[repr(C)]
pub struct UninitializedSendable<T> {
    contents: NonNull<T>,
}

pub struct Sendable<T> {
    contents: *mut T,
    total_size: usize,
    memory_state: MemoryState,
    // NOTE: this marker has no consequences for variance, but is necessary
    // for dropck to understand that we logically own a `T`.
    //
    // For details, see:
    // https://github.com/rust-lang/rfcs/blob/master/text/0769-sound-generic-drop.md#phantom-data
    _marker: PhantomData<T>,
}

impl<T> UninitializedSendable<T> {
    fn type_size() -> usize {
        let type_size = core::mem::size_of::<T>();
        let remainder = type_size & 4095;
        type_size + (4096 - remainder)
    }

    pub fn new(val: T) -> Result<UninitializedSendable<T>, crate::Error>
    where
        T: Unpin,
    {
        let uninitialized = Self::uninit()?;
        let type_size = core::mem::size_of::<T>();
        unsafe {
            let src_slice = core::slice::from_raw_parts(&val as *const _ as *const u8, type_size);
            let dest_slice = core::slice::from_raw_parts_mut(
                uninitialized.contents.as_ptr() as *mut u8,
                type_size,
            );
            for (src, dest) in src_slice.iter().zip(dest_slice) {
                *dest = *src;
            }
        }
        Ok(uninitialized)
    }

    pub fn uninit() -> Result<UninitializedSendable<T>, crate::Error> {
        let padded_size = Self::type_size();

        // Ensure this object takes up exactly a multiple of a page. This
        // ensures it can be sent to another process.
        let new_mem = crate::map_memory(
            None,
            None,
            padded_size,
            crate::MemoryFlags::R | crate::MemoryFlags::W,
        )?;

        let contents = unsafe { NonNull::new_unchecked(new_mem.as_mut_ptr() as *mut T) };
        Ok(UninitializedSendable { contents })
    }

    pub unsafe fn assume_init(self) -> Sendable<T> {
        Sendable {
            contents: self.contents.as_ptr(),
            total_size: Self::type_size(),
            memory_state: MemoryState::Available,
            _marker: PhantomData,
        }
    }
}

impl<T: Default + Unpin + Send> Default for Sendable<T> {
    /// Creates a `Box<T>`, with the `Default` value for T.
    fn default() -> Sendable<T> {
        Sendable::new(Default::default()).unwrap()
    }
}

// impl<T> Default for Sendable<[T]> {
//     fn default() -> Sendable<[T]> {
//         Sendable::<[T; 0]>::new([])
//     }
// }

/// `Unique` pointers are `Send` if `T` is `Send` because the data they
/// reference is unaliased. Note that this aliasing invariant is
/// unenforced by the type system; the abstraction using the
/// `Unique` must enforce it.
unsafe impl<T: Send> Send for Sendable<T> {}

/// `Unique` pointers are `Sync` if `T` is `Sync` because the data they
/// reference is unaliased. Note that this aliasing invariant is
/// unenforced by the type system; the abstraction using the
/// `Unique` must enforce it.
unsafe impl<T: Sync> Sync for Sendable<T> {}

impl<T: Send> Sendable<T> {
    pub fn new(val: T) -> Result<Sendable<T>, crate::Error>
    where
        T: Unpin,
    {
        Ok(unsafe { UninitializedSendable::new(val)?.assume_init() })
    }

    /// Perform an immutable lend of this Carton to the specified server.
    /// This function will block until the server returns.
    pub fn lend(&self, connection: crate::CID, id: u32) -> Result<crate::Result, crate::Error> {
        let buf = crate::MemoryRange::new(self.contents as usize, self.total_size)?;
        let msg = crate::MemoryMessage {
            id: id as usize,
            buf,
            offset: None,
            valid: crate::MemorySize::new(core::mem::size_of::<T>()),
        };
        crate::send_message(connection, crate::Message::Borrow(msg))
    }

    /// Perform a mutable lend of this Carton to the server.
    pub fn lend_mut(
        &mut self,
        connection: crate::CID,
        id: u32,
    ) -> Result<crate::Result, crate::Error> {
        let buf = crate::MemoryRange::new(self.contents as usize, self.total_size)?;
        let msg = crate::MemoryMessage {
            id: id as usize,
            buf,
            offset: None,
            valid: crate::MemorySize::new(core::mem::size_of::<T>()),
        };
        self.memory_state = MemoryState::MutablyLent;
        let result = crate::send_message(connection, crate::Message::MutableBorrow(msg));
        self.memory_state = MemoryState::Available;
        result
    }

    /// Perform a move of this Carton to the server.
    pub fn send(
        mut self,
        connection: crate::CID,
        id: u32,
    ) -> Result<crate::Result, crate::Error> {
        let buf = crate::MemoryRange::new(self.contents as usize, self.total_size)?;
        let msg = crate::MemoryMessage {
            id: id as usize,
            buf,
            offset: None,
            valid: crate::MemorySize::new(core::mem::size_of::<T>()),
        };
        let result = crate::send_message(connection, crate::Message::Move(msg))?;

        // Mark this state as Moved, which prevents it from being Dropped.
        self.memory_state = MemoryState::Moved;
        Ok(result)
    }
}

impl<T> Deref for Sendable<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.contents }
    }
}

impl<T> DerefMut for Sendable<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.contents }
    }
}

// Display formatting
impl<T: fmt::Display> fmt::Display for Sendable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug> fmt::Debug for Sendable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T> fmt::Pointer for Sendable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // It's not possible to extract the inner Uniq directly from the Box,
        // instead we cast it to a *const which aliases the Unique
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

impl<T: PartialEq> PartialEq for Sendable<T> {
    #[inline]
    fn eq(&self, other: &Sendable<T>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
    #[inline]
    fn ne(&self, other: &Sendable<T>) -> bool {
        PartialEq::ne(&**self, &**other)
    }
}

impl<T: PartialOrd> PartialOrd for Sendable<T> {
    #[inline]
    fn partial_cmp(&self, other: &Sendable<T>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
    #[inline]
    fn lt(&self, other: &Sendable<T>) -> bool {
        PartialOrd::lt(&**self, &**other)
    }
    #[inline]
    fn le(&self, other: &Sendable<T>) -> bool {
        PartialOrd::le(&**self, &**other)
    }
    #[inline]
    fn ge(&self, other: &Sendable<T>) -> bool {
        PartialOrd::ge(&**self, &**other)
    }
    #[inline]
    fn gt(&self, other: &Sendable<T>) -> bool {
        PartialOrd::gt(&**self, &**other)
    }
}

impl<T: Ord> Ord for Sendable<T> {
    #[inline]
    fn cmp(&self, other: &Sendable<T>) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T: Eq> Eq for Sendable<T> {}

impl<T: Hash> Hash for Sendable<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T> Drop for Sendable<T> {
    fn drop(&mut self) {
        if self.memory_state != MemoryState::Available {
            panic!("invalid memory state: {:?}", self.memory_state);
        }
        let range = crate::MemoryRange::new(self.contents as usize, self.total_size).unwrap();
        crate::unmap_memory(range).unwrap();
    }
}
