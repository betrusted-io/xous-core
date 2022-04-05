#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod dns;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use dns::*;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub mod dns_hosted;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub use dns_hosted::*;

pub mod ping;
pub use ping::*;

#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod tcp_stream;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use tcp_stream::*;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod tcp_listener;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use tcp_listener::*;

#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub mod tcp_hosted;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub use tcp_hosted::*;
