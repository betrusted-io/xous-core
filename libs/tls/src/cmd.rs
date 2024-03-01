use std::convert::TryFrom;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::str::from_utf8;
use std::sync::Arc;

use locales::t;
use rustls::pki_types::ServerName;
#[cfg(feature = "rootCA")]
use {modals::Modals, std::convert::TryInto, xous_names::XousNames};

use crate::Tls;

pub fn shellchat<'a>(mut tokens: impl Iterator<Item = &'a str>) -> Result<Option<String>, xous::Error> {
    use core::fmt::Write;
    let mut ret = String::new();
    match tokens.next() {
        // delete ALL trusted CA Certificates
        Some("deleteall") => {
            log::info!("starting TLS delete certificates");
            let tls = Tls::new();
            let count = tls.del_all_rota().unwrap();
            write!(ret, "{} {}", count, t!("tls.deleteall_done", locales::LANG)).ok();
            log::info!("finished TLS delete certificates");
        }
        // helpful stuff
        Some("help") => {
            write!(ret, "{}", t!("tls.cmd_help", locales::LANG)).ok();
        }
        // list trusted Certificate Authority certificates
        Some("list") => {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("starting TLS trusted listing");
            let tls = Tls::new();
            for ota in tls.trusted() {
                write!(ret, "🏛 {}\n", ota).ok();
            }
            log::info!("finished TLS trusted listing");
        }
        // save/trust all Root CA's in webpki-roots en-masse
        #[cfg(feature = "rootCA")]
        Some("mozilla") => {
            let xns = XousNames::new().unwrap();
            let modals = Modals::new(&xns).unwrap();
            let mut count: u32 = webpki_roots::TLS_SERVER_ROOTS.len().try_into().unwrap();
            modals
                .start_progress(t!("tls.mozilla_progress", locales::LANG), 0, count, 0)
                .expect("no progress");
            count = 0;
            let tls = Tls::new();
            for ta in webpki_roots::TLS_SERVER_ROOTS {
                let ota = crate::OwnedTrustAnchor::from(ta);
                tls.save_ta(&ota).unwrap_or_else(|e| log::warn!("{e}"));
                modals.update_progress(count).expect("no progress");
                count += 1;
            }
            modals.finish_progress().expect("finish progress");
            write!(ret, "{} {}", count, t!("tls.mozilla_done", locales::LANG)).ok();
        }
        // inspect establishes a tls connection to the supplied host, extracts the
        // certificates offered and immediately closes the connection.
        // The certificates are presented by modal to the user, and saved to the
        // pddb if trusted.
        Some("inspect") => {
            log::set_max_level(log::LevelFilter::Info);
            let target = match tokens.next() {
                Some(target) => target,
                None => "betrusted.io",
            };
            let tls = Tls::new();
            match tls.inspect(target) {
                Ok(count) => write!(ret, "{} {}", count, t!("tls.inspect_done", locales::LANG)).ok(),
                Err(_) => write!(ret, "{} {target}", t!("tls.inspect_fail_servername", locales::LANG)).ok(),
            };
            log::set_max_level(log::LevelFilter::Info);
        }

        Some("test") => {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("starting TLS run");
            log::info!("build TLS client config");
            let tls = Tls::new();
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(tls.root_store())
                .with_no_client_auth();
            let target = match tokens.next() {
                Some(target) => target,
                None => "bunnyfoo.com",
            };
            log::info!("point TLS to {}", target);
            log::info!("connect TCPstream to {}", target);
            match TcpStream::connect((target, 443)) {
                Ok(mut sock) => {
                    log::info!("tcp connected");
                    write!(ret, "{}", t!("tls.test_success_tcp", locales::LANG)).ok();
                    match ServerName::try_from(target.to_owned()) {
                        Ok(server_name) => {
                            match rustls::ClientConnection::new(Arc::new(config), server_name) {
                                Ok(mut conn) => {
                                    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
                                    log::info!("create http headers and write to server");
                                    match tls.write_all(b"GET / HTTP/1.1\r\n\r\n") {
                                        Ok(()) => {
                                            log::info!("tls accepted GET");
                                            write!(ret, "{}", t!("tls.test_success_get", locales::LANG)).ok();
                                            let mut plaintext = Vec::new();
                                            log::info!("read TLS response");
                                            match tls.read_to_end(&mut plaintext) {
                                                Ok(n) => {
                                                    log::info!("tls received {} bytes", n);
                                                    write!(
                                                        ret,
                                                        "{} {}\n",
                                                        t!("tls.test_success_bytes", locales::LANG),
                                                        n
                                                    )
                                                    .ok();
                                                    log::info!(
                                                        "{}",
                                                        from_utf8(&plaintext).unwrap_or("utf-error")
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
                                    log::warn!("failed to construct ClientConnection: {e}");
                                    write!(ret, "{e}\n").ok();
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("failed to convert target into a valid ServerName: {e}");
                            write!(ret, "{e}\n").ok();
                        }
                    }
                }
                Err(e) => {
                    log::warn!("failed to connect tcp: {e}");
                    write!(ret, "{e}\n").ok();
                }
            };

            log::set_max_level(log::LevelFilter::Info);
        }
        None | _ => {
            write!(ret, "{}\n", t!("tls.cmd", locales::LANG)).ok();
            write!(ret, "\tdeleteall\t{}\n", t!("tls.deleteall_cmd", locales::LANG)).ok();
            write!(ret, "\thelp\n").ok();
            write!(ret, "\tlist\t{}\n", t!("tls.list_cmd", locales::LANG)).ok();
            #[cfg(feature = "rootCA")]
            write!(ret, "\tmozilla\t{}\n", t!("tls.mozilla_cmd", locales::LANG)).ok();
            write!(ret, "\tinspect <host>\t{}\n", t!("tls.inspect_cmd", locales::LANG)).ok();
            write!(ret, "\ttest <host>\t{}\n", t!("tls.test_cmd", locales::LANG)).ok();
        }
    }
    Ok(Some(ret))
}
