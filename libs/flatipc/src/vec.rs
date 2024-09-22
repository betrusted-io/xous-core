use std::mem::MaybeUninit;

pub struct Vec<T, const N: usize> {
    length: usize,
    buffer: [MaybeUninit<T>; N],
}

unsafe impl<T, const N: usize> crate::IpcSafe for Vec<T, N> {}

impl<T, const N: usize> Vec<T, N> {
    pub fn new() -> Self {
        let buffer = [const { MaybeUninit::uninit() }; N];
        Vec { buffer, length: 0 }
    }

    pub fn push(&mut self, value: T) {
        if self.length < N {
            self.buffer[self.length] = MaybeUninit::new(value);
            self.length += 1;
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.length > 0 {
            self.length -= 1;
            unsafe { Some(self.buffer[self.length].as_ptr().read()) }
        } else {
            None
        }
    }

    pub fn len(&self) -> usize { self.length }

    pub fn as_slice(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.buffer.as_ptr() as *const T, self.length) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer.as_mut_ptr() as *mut T, self.length) }
    }

    pub fn clear(&mut self) {
        for i in 0..self.length {
            unsafe {
                core::ptr::drop_in_place(&mut self.buffer[i]);
            }
        }
        self.length = 0;
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> { self.as_slice().iter() }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> { self.as_mut_slice().iter_mut() }

    pub fn resize(&mut self, new_len: usize, value: T)
    where
        T: Clone,
    {
        if new_len > self.length {
            for _ in self.length..new_len {
                self.push(value.clone());
            }
        } else {
            self.length = new_len;
        }
    }
}

impl<T, const N: usize> Drop for Vec<T, N> {
    fn drop(&mut self) { self.clear(); }
}

impl<T, const N: usize> core::fmt::Debug for Vec<T, N>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> Default for Vec<T, N> {
    fn default() -> Self { Vec::new() }
}

impl<T, const N: usize> core::ops::Deref for Vec<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target { self.as_slice() }
}

impl<T, const N: usize> core::ops::DerefMut for Vec<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.as_mut_slice() }
}

impl<T, const N: usize> From<&[T]> for Vec<T, N>
where
    T: Clone,
{
    fn from(value: &[T]) -> Self {
        let mut vec = Vec::new();
        for item in value {
            vec.push(item.clone());
        }
        vec
    }
}

impl<T, const N: usize> From<&mut [T]> for Vec<T, N>
where
    T: Clone,
{
    fn from(value: &mut [T]) -> Self {
        let mut vec = Vec::new();
        for item in value {
            vec.push(item.clone());
        }
        vec
    }
}

impl<T, const N: usize> core::fmt::Display for Vec<T, N>
where
    T: core::fmt::Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, item) in self.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", item)?;
        }
        Ok(())
    }
}

impl<T, const N: usize> core::ops::Index<usize> for Vec<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output { &self.as_slice()[index] }
}

impl<T, const N: usize> core::ops::IndexMut<usize> for Vec<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.as_mut_slice()[index] }
}
