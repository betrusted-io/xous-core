// pub mod syscall;
// pub use syscall::*;
pub fn context_to_args(call: usize, _init: ContextInit) -> [usize; 8] {
    [
        SysCallNumber::CreateThread as usize,
        a1.get(),
        a2.get(),
        a3.map(|x| x.get()).unwrap_or_default(),
        0,
        0,
        0,
        0,
    ]
}

pub fn args_to_context(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> core::result::Result<ContextInit, crate::Error> {
    Ok((
        MemoryAddress::new(a1).ok_or(Error::InvalidSyscall)?,
        MemoryAddress::new(a2).ok_or(Error::InvalidSyscall)?,
        MemoryAddress::new(a3),
    ))
}
