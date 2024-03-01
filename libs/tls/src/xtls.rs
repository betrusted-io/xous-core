use std::{convert::TryFrom, fmt::Debug, io, net::TcpStream, result::Result, sync::Arc};

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use ureq::{ReadWrite, Response};

use crate::Tls;

pub struct TlsConnector {}

/// Set up tls with rustls::ClientConnection,
/// BUT - on Error::InvalidCertificate - then
/// probe the host for the untrusted certificate chain and prompt the user
/// to perhaps trust one of the certificates in the chain - then try again.
impl ureq::TlsConnector for TlsConnector {
    fn connect(&self, dns_name: &str, mut io: Box<dyn ReadWrite>) -> Result<Box<dyn ReadWrite>, ureq::Error> {
        log::info!("Commencing tls connection setup");
        match ServerName::try_from(dns_name.to_owned()) {
            Ok(server_name) => {
                loop {
                    // refresh rustls client config with current root_store
                    let tls = Tls::new();
                    let config = rustls::ClientConfig::builder()
                        .with_root_certificates(tls.root_store())
                        .with_no_client_auth();
                    match rustls::ClientConnection::new(Arc::new(config), server_name.clone()) {
                        Ok(mut connection) => {
                            log::info!("tls handshake started");
                            match connection.complete_io(&mut io) {
                                Ok(_) => {
                                    if connection.peer_certificates().is_some() {
                                        return Ok(Box::new(TlsStream(StreamOwned::new(connection, io))));
                                    }
                                }
                                // errors generated late in the tls handshake
                                Err(e) => {
                                    if let Some(inner) = e.get_ref() {
                                        if let Some(rustls_error) = inner.downcast_ref::<rustls::Error>() {
                                            if let rustls::Error::InvalidCertificate(_) = rustls_error {
                                                if let Ok(certs) = tls.probe(dns_name) {
                                                    if certs.len() > 0 {
                                                        log::info!("try again with new trusted certs");
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // non certificate chain errors
                                    log::warn!("{e}");
                                    break;
                                }
                            }
                        }
                        // errors generated early in the tls handshake
                        Err(rustls::Error::InvalidCertificate(_)) => {
                            if let Ok(certs) = tls.probe(dns_name) {
                                if certs.len() > 0 {
                                    log::info!("try again with new trusted certs");
                                    continue;
                                }
                            }
                        }
                        // non certificate chain errors
                        Err(e) => {
                            log::warn!("{e}");
                            break;
                        }
                    }
                }
                log::warn!("failed to establish tls connection");
                // this would be better as a ureq:Error::Transport but they are hard to build
                Err(ureq::Error::Status(
                    526,
                    Response::new(526, "tls", "untrusted certificate chain").unwrap(),
                ))
            }
            Err(e) => {
                log::warn!("failed to convert dns_name into a valid server name: {e}");
                Err(ureq::Error::Status(
                    400,
                    Response::new(526, "http", "failed to convert dns_name into a valid server name")
                        .unwrap(),
                ))
            }
        }
    }
}

// TlsStream wraps StreamOwned and implements ReadWrite for use in TlsConnect::connect()
#[derive(Debug)]
pub struct TlsStream(StreamOwned<ClientConnection, Box<dyn ReadWrite>>);

impl TlsStream {
    /// Returns a shared reference to the inner stream.
    pub fn get_ref(&self) -> &Box<dyn ReadWrite> { self.0.get_ref() }

    /// Returns a mutable reference to the inner stream.
    pub fn get_mut(&mut self) -> &mut Box<dyn ReadWrite> { self.0.get_mut() }
}

impl io::Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> { Ok(self.0.read(buf)?) }
}

impl io::Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> { Ok(self.0.write(buf)?) }

    fn flush(&mut self) -> Result<(), std::io::Error> { Ok(self.0.flush()?) }
}

// required for the return type in TlsConnect::connect()
impl ReadWrite for TlsStream {
    fn socket(&self) -> Option<&TcpStream> { self.get_ref().socket() }
}
