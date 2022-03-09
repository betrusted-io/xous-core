#![cfg_attr(target_os = "none", no_std)]
#![allow(nonstandard_style)]

pub type c_char = i8;
pub type c_schar = i8;
pub type c_uchar = u8;
pub type c_short = i16;
pub type c_ushort = u16;
pub type c_int = i32;
pub type c_uint = u32;
pub type c_long = i32;
pub type c_ulong = u32;
pub type c_longlong = i64;
pub type c_ulonglong = u64;
pub type c_float = f32;
pub type c_double = f64;
pub type c_void = core::ffi::c_void;

extern "C" {
    pub fn add_one(a: i32) -> i32;
}

extern "C" {
    pub fn malloc_test() -> i32;
}

static mut PUTC_BUF: Vec::<u8> = Vec::new();
#[export_name = "libc_putchar"]
pub unsafe extern "C" fn libc_putchar(
    c: c_char,
) {
    let char = c as u8;
    if char != 0xa && char != 0xd {
        PUTC_BUF.push(char);
    } else {
        let s = String::from_utf8_lossy(&PUTC_BUF);
        log::info!("ffi-test: {}", s);
        PUTC_BUF.clear();
    }
}

static mut C_HEAP: Vec::<Vec::<u8>> = Vec::new();
#[export_name = "malloc"]
pub unsafe extern "C" fn malloc(
    size: c_uint
) -> *mut c_void {
    // note: we might need to use `Pin` to keep the data from moving around in the heap, if we see weird behavior
    // happening
    let checked_size = if size == 0 {
        1 // at least 1 element so we can get a pointer to pass back
    } else {
        size
    };
    let mut alloc: Vec::<u8> = Vec::with_capacity(checked_size as usize);
    for _ in 0..checked_size {
        alloc.push(0);
    }
    let ptr = alloc.as_mut_ptr();
    // store a reference to the allocated vector, under the theory that this keeps it from going out of scope
    C_HEAP.push(alloc);
    log::info!("allocated: {:x}({})#{}", ptr as usize, size, C_HEAP.len());

    ptr as *mut c_void
}

#[export_name = "free"]
pub unsafe extern "C" fn free(
    ptr: *mut c_void
) {
    let mut region_index: Option<usize> = None;
    for (index, region) in C_HEAP.iter().enumerate() {
        if region.as_ptr() as usize == ptr as usize {
            region_index = Some(index);
            break;
        }
    }
    match region_index {
        Some(index) => {
            let mut removed = C_HEAP.remove(index);
            log::info!("free success: {:x}({})#{}", ptr as usize, removed.len(), C_HEAP.len());
            removed.clear();
        }
        None => {
            log::info!("free failed, debug! Requested free: {:x}", ptr as usize);
            for region in C_HEAP.iter() {
                log::info!("  {:x}({})", region.as_ptr() as usize, region.len());
            }
        }
    }
}

#[export_name = "realloc"]
pub unsafe extern "C" fn realloc(
    ptr: *mut c_void,
    size: c_uint
) -> *mut c_void {
    let mut region_index: Option<usize> = None;
    for (index, region) in C_HEAP.iter().enumerate() {
        if region.as_ptr() as usize == ptr as usize {
            region_index = Some(index);
            break;
        }
    }
    match region_index {
        Some(index) => {
            log::info!("realloc/free success: {:x}", ptr as usize);
            let mut old = C_HEAP.remove(index);
            let checked_size = if size == 0 {
                1 // at least 1 element so we have a pointer we can pass back
            } else {
                size
            };
            let mut alloc: Vec::<u8> = Vec::with_capacity(checked_size as usize);
            let ret_ptr = alloc.as_mut_ptr();
            for &src in old.iter() {
                alloc.push(src);
            }
            old.clear();
            C_HEAP.push(alloc);
            log::info!("realloc/allocated: {:x}", ret_ptr as usize);

            ret_ptr as *mut c_void
        }
        None => {
            log::info!("realloc failed, debug! Requested realloc: {:x}({})", ptr as usize, size);
            for region in C_HEAP.iter() {
                log::info!("  {:x}({})", region.as_ptr() as usize, region.len());
            }
            return ::core::ptr::null::<c_void>() as *mut c_void
        }
    }
}
