use rustls::{ClientConnection, StreamOwned};
use std::io::{Error, ErrorKind};
use std::net::TcpStream;
use tls::Tls;
use tungstenite::WebSocket;

const PROVISIONING_PATH: &str = "/v1/websocket/provisioning/";
const REGISTRATION_PATH: &str = "/v1/registration";

#[allow(dead_code)]
pub enum SignalWS {
    Message {
        ws: WebSocket<StreamOwned<ClientConnection, TcpStream>>,
    },
    Provision {
        ws: WebSocket<StreamOwned<ClientConnection, TcpStream>>,
    },
    Register {
        ws: WebSocket<StreamOwned<ClientConnection, TcpStream>>,
    },
}

impl SignalWS {
    #[allow(dead_code)]
    pub fn message(_host: &str) -> Result<Self, Error> {
        todo!();
    }

    pub fn provision(host: &str) -> Result<Self, Error> {
        let server = format!("wss://{host}{}", PROVISIONING_PATH);
        // let mut conn = client_connection(&server);
        match SignalWS::connect(server) {
            Ok(ws) => Ok(Self::Provision { ws }),
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)]
    pub fn register(host: &str) -> Result<Self, Error> {
        let server = format!("wss://{host}{}", REGISTRATION_PATH);
        // let mut conn = client_connection(&server);
        match SignalWS::connect(server) {
            Ok(ws) => Ok(Self::Register { ws }),
            Err(e) => Err(e),
        }
    }

    pub fn close(&mut self) {
        match self {
            SignalWS::Message { ws, .. }
            | SignalWS::Provision { ws, .. }
            | SignalWS::Register { ws, .. } => {
                ws.close(None)
                    .unwrap_or_else(|e| log::warn!("failed to close websocket: {e}"));
                // TODO close properly https://docs.rs/tungstenite/0.20.1/tungstenite/protocol/struct.WebSocket.html
                ws.flush()
                    .unwrap_or_else(|e| log::warn!("failed to flush websocket: {e}"));
            }
        }
    }

    /// Make a websocket connection to host server
    ///
    /// # Arguments
    /// * `server` - server
    ///
    /// # Returns
    ///
    fn connect(
        server: String,
    ) -> Result<WebSocket<StreamOwned<ClientConnection, TcpStream>>, Error> {
        log::info!("connecting websocket");
        match url::Url::parse(&server) {
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