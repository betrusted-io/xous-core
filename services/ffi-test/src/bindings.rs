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

use xous::{send_message, Message};
static mut KBD: Option<keyboard::Keyboard> = None;
fn get_keys_blocking() -> Vec<char> {
    if unsafe { KBD.is_none() } {
        let xns = xous_names::XousNames::new().unwrap();
        let kbd = keyboard::Keyboard::new(&xns).unwrap();
        unsafe { KBD = Some(kbd) };
    }
    match send_message(
        unsafe { KBD.as_ref().unwrap().conn() },
        Message::new_blocking_scalar(
            9, // BlockingKeyboardListener
            0, 0, 0, 0,
        ),
    ) {
        Ok(xous::Result::Scalar2(k1, k2)) => {
            let mut ret = Vec::<char>::new();
            if let Some(c) = core::char::from_u32(k1 as u32) {
                ret.push(c)
            }
            if let Some(c) = core::char::from_u32(k2 as u32) {
                ret.push(c)
            }
            ret
        }
        Ok(_) | Err(_) => panic!("internal error: Incorrect return type"),
    }
}

#[export_name = "rust_getchar"]
pub unsafe extern "C" fn rust_getchar() -> c_char {
    log::info!("rust_getchar called");
    let mut chr = 0 as c_char;
    let kbhit = get_keys_blocking();
    for k in kbhit {
        log::info!("got key character: {}", k);
        chr = k as i8;
        return chr;
        //break;
    }
    log::info!("libc_getchar cleanup FIXME");
    //kbd.drop(); ?????
    return chr;
}

/*
/// Call this from your Rust init routine, before using get_keys_blocking()
pub fn init_kbd() {
    let xns = xous_names::XousNames::new().unwrap();
    let kbd = keyboard::Keyboard::new(&xns).unwrap();
    KBD_CONN.store(kbd.conn(), Ordering::SeqCst);
    unsafe{KBD = Some(kbd)};
}*/

extern "C" {
    pub fn add_one(a: i32) -> i32;
}

extern "C" {
    pub fn malloc_test() -> i32;
}

static mut PUTC_BUF: Vec<u8> = Vec::new();
#[export_name = "libc_putchar"]
pub unsafe extern "C" fn libc_putchar(c: c_char) {
    let char = c as u8;
    if char != 0xa && char != 0xd {
        PUTC_BUF.push(char);
    } else {
        let s = String::from_utf8_lossy(&PUTC_BUF);
        log::info!("ffi-test: {}", s);
        PUTC_BUF.clear();
    }
}

static mut C_HEAP: Vec<Vec<u8>> = Vec::new();
#[export_name = "malloc"]
pub unsafe extern "C" fn malloc(size: c_uint) -> *mut c_void {
    // note: we might need to use `Pin` to keep the data from moving around in the heap, if we see weird
    // behavior happening
    let checked_size = if size == 0 {
        1 // at least 1 element so we can get a pointer to pass back
    } else {
        size
    };
    let mut alloc: Vec<u8> = Vec::with_capacity(checked_size as usize);
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
pub unsafe extern "C" fn free(ptr: *mut c_void) {
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
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: c_uint) -> *mut c_void {
    if ptr.is_null() {
        // if ptr is null, realloc() is identical to malloc()
        return malloc(size);
    }
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
            let mut old = C_HEAP.swap_remove(index);
            let checked_size = if size == 0 {
                1 // at least 1 element so we have a pointer we can pass back
            } else {
                size
            };
            let mut alloc: Vec<u8> = Vec::with_capacity(checked_size as usize);
            let ret_ptr = alloc.as_mut_ptr();
            for &src in old.iter() {
                alloc.push(src);
            }
            old.clear();
            alloc.set_len(checked_size as usize);
            C_HEAP.push(alloc);
            log::info!("realloc/allocated: {:x}", ret_ptr as usize);

            ret_ptr as *mut c_void
        }
        None => {
            log::info!("realloc failed, debug! Requested realloc: {:x}({})", ptr as usize, size);
            for region in C_HEAP.iter() {
                log::info!("  {:x}({})", region.as_ptr() as usize, region.len());
            }
            return ::core::ptr::null::<c_void>() as *mut c_void;
        }
    }
}
