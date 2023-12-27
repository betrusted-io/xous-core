use rustls::{ClientConnection, StreamOwned};
use std::io::{Error, ErrorKind};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use tls::Tls;
use tungstenite::{Message, WebSocket};

const PROVISIONING_PATH: &str = "/v1/websocket/provisioning/";
const REGISTRATION_PATH: &str = "/v1/registration";

#[allow(dead_code)]
pub struct SignalWS {
    ws: Arc<Mutex<WebSocket<StreamOwned<ClientConnection, TcpStream>>>>,
}

impl SignalWS {
    #[allow(dead_code)]
    pub fn new_message(_host: &str) -> Result<Self, Error> {
        todo!();
    }

    fn new(server: &str) -> Result<Self, Error> {
        match SignalWS::connect(server) {
            Ok(ws) => Ok(Self {
                ws: Arc::new(Mutex::new(ws)),
            }),
            Err(e) => Err(e),
        }
    }

    pub fn new_provision(host: &str) -> Result<Self, Error> {
        Ok(Self::new(&format!("wss://{host}{}", PROVISIONING_PATH))?)
    }

    #[allow(dead_code)]
    pub fn new_register(host: &str) -> Result<Self, Error> {
        Ok(Self::new(&format!("wss://{host}{}", REGISTRATION_PATH))?)
    }

    pub fn close(&mut self) {
        if let Ok(mut ws) = self.ws.lock() {
            ws.close(None)
                .unwrap_or_else(|e| log::warn!("failed to close websocket: {e}"));
            // TODO close properly https://docs.rs/tungstenite/0.20.1/tungstenite/protocol/struct.WebSocket.html
            ws.flush()
                .unwrap_or_else(|e| log::warn!("failed to flush websocket: {e}"));
        };
    }

    pub fn read(&mut self) -> Result<Message, Error> {
        let msg = if let Ok(mut ws) =self. ws.lock() {
            match ws.read() {
                Ok(msg) => Ok(msg),
                Err(e) => {
                    log::warn!("{e}");
                    Err(Error::new(ErrorKind::Other, "error on reading ws"))
                }
            }
        } else {
            Err(Error::new(
                ErrorKind::Other,
                "failed to get lock on websocket",
            ))
        };
        msg
    }

    /// Make a websocket connection to host server
    ///
    /// # Arguments
    /// * `server` - server
    ///
    /// # Returns
    ///
    fn connect(server: &str) -> Result<WebSocket<StreamOwned<ClientConnection, TcpStream>>, Error> {
        log::info!("connecting websocket");
        match url::Url::parse(server) {
            Ok(url) => {
                log::info!("attempting websocket connection to {}", url.as_str());
                let host = url.host_str().expect("failed to extract host from url");
                match TcpStream::connect((host, 443)) {
                    Ok(sock) => {
                        log::info!("tcp connected to {host}");
                        let xtls = Tls::new();
                        match xtls.stream_owned(host, sock) {
                            Ok(tls_stream) => {
                                log::info!("tls configured");
                                match tungstenite::client(url, tls_stream) {
                                    Ok((socket, response)) => {
                                        log::info!("Websocket connected to: {}", server);
                                        log::info!("Response HTTP code: {}", response.status());
                                        Ok(socket)
                                    }
                                    Err(e) => {
                                        log::info!("failed to connect websocket: {}", e);
                                        Err(Error::from(ErrorKind::ConnectionRefused))
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("failed to configure tls: {e}");
                                Err(e)
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("failed to connect tcp: {e}");
                        Err(e)
                    }
                }
            }
            Err(e) => {
                log::info!("failed to parse server url: {e}");
                Err(Error::new(ErrorKind::InvalidData, e))
            }
        }
    }
}
