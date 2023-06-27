use crate::{check_trust, danger, trusted::Trusted};
use std::convert::TryInto;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;

pub fn shellchat<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
) -> Result<Option<String>, xous::Error> {
    use core::fmt::Write;
    let mut ret = String::new();
    match tokens.next() {
        // save/trust all Root CA's in webpki-roots en-masse
        #[cfg(feature = "rootCA")]
        Some("mozilla") => {
            let rotas: Vec<crate::RustlsOwnedTrustAnchor> = webpki_roots::TLS_SERVER_ROOTS
                .0
                .iter()
                .map(|ta| {
                    crate::RustlsOwnedTrustAnchor::from_subject_spki_name_constraints(
                        ta.subject,
                        ta.spki,
                        ta.name_constraints,
                    )
                })
                .collect();
            let mut i = 0;
            for rota in rotas {
                crate::save_cert(format!("webpki-root-{i}").as_str(), &rota).unwrap();
                i += 1;
            }
        }
        // probe establishes a tls connection to the supplied host, extracts the
        // certificates offered and immediately closes the connection.
        // The certificates are presented by modal to the user, and saved to the
        // pddb if trusted.
        Some("probe") => {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("starting TLS probe");
            // Attempt to open the tls connection with an empty root_store
            let root_store = rustls::RootCertStore::empty();
            // Stifle the default rustls certificate verification's complaint about an
            // unknown/untrusted CA root certificate so that we get to see the certificate chain
            let stifled_verifier =
                Arc::new(danger::StifledCertificateVerification { roots: root_store });
            let config = rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(stifled_verifier)
                .with_no_client_auth();
            let target = match tokens.next() {
                Some(target) => target,
                None => "bunnyfoo.com",
            };
            let server_name = target.try_into().unwrap_or_else(|e| {
                log::warn!("failed to create sever_name from {target}: {e}");
                write!(ret, "failed to create sever_name from {target}").ok();
                "bunnyfoo.com".try_into().unwrap()
            });
            let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();
            log::info!("connect TCPstream to {}", target);
            let url = format!("{}:443", target);
            match TcpStream::connect(url) {
                Ok(mut sock) => {
                    match conn.complete_io(&mut sock) {
                        Ok(_) => log::info!("handshake complete"),
                        Err(e) => {
                            write!(ret, "{e}").ok();
                            log::warn!("{e}");
                        }
                    }
                    conn.send_close_notify();

                    match conn.peer_certificates() {
                        Some(certificates) => check_trust(certificates),
                        None => false,
                    };
                }
                Err(e) => {
                    write!(ret, "{e}").ok();
                    log::warn!("{e}")
                }
            };

            log::set_max_level(log::LevelFilter::Info);
        }

        Some("test") => {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("starting TLS run");
            log::info!("build TLS client config");
            let trusted = Trusted::new().unwrap();
            let config = rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(trusted.into())
                .with_no_client_auth();
            let target = match tokens.next() {
                Some(target) => target,
                None => "bunnyfoo.com",
            };
            log::info!("point TLS to {}", target);
            let mut conn =
                rustls::ClientConnection::new(Arc::new(config), target.try_into().unwrap())
                    .unwrap();

            log::info!("connect TCPstream to {}", target);
            let url = format!("{}:443", target);
            match TcpStream::connect(url) {
                Ok(mut sock) => {
                    log::info!("tcp connected");
                    write!(ret, "tcp connected\n").ok();
                    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
                    log::info!("create http headers and write to server");
                    let msg = format!("GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept-Encoding: identity\r\n\r\n", target);
                    match tls.write_all(msg.as_bytes()) {
                        Ok(()) => {
                            log::info!("tls accepted GET");
                            write!(ret, "tls accepted GET\n").ok();
                            let mut plaintext = Vec::new();
                            log::info!("read TLS response");
                            match tls.read_to_end(&mut plaintext) {
                                Ok(n) => {
                                    log::info!("tls received {} bytes", n);
                                    write!(ret, "tls received {} bytes\n", n).ok();
                                    log::info!(
                                        "{}",
                                        std::str::from_utf8(&plaintext).unwrap_or("utf-error")
                                    );
                                }
                                Err(e) => {
                                    log::warn!("failed to read tls response: {e}");
                                    write!(ret, "{e}\n").ok();
                                }
                            };
                        }
                        Err(e) => {
                            log::warn!("failed to GET on tls connection: {e}");
                            write!(ret, "{e}\n").ok();
                        }
                    };
                }
                Err(e) => {
                    log::warn!("failed to connect tcp: {e}");
                    write!(ret, "{e}\n").ok();
                }
            };

            log::set_max_level(log::LevelFilter::Info);
        }
        // list trusted Certificate Authority certificates
        Some("trusted") => {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("starting TLS trusted listing");
            let trusted = Trusted::new().expect("failed to initiate trusted iterator");
            for rota in trusted {
                write!(ret, "🏛 {}\n", rota.subject()).ok();
            }
            log::info!("finished TLS trusted listing");
        }
        None | _ => {
            write!(ret, "net tls <sub-command>\n").ok();
            #[cfg(feature = "rootCA")]
            write!(ret, "\tmozilla\ttrust all Root CA's in webpki-roots\n").ok();
            write!(ret, "\tprobe <host>\tsave host CA'a if trusted\n").ok();
            write!(ret, "\ttest <host>\tmake tls connection to host\n").ok();
            write!(ret, "\ttrusted\tlist trusted CA certificates\n").ok();
        }
    }
    Ok(Some(ret))
}
