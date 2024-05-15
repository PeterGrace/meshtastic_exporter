use std::net::SocketAddr;
use meshtastic::protobufs::{FromRadio, ToRadio};
use serde::Deserialize;

#[derive(Debug)]
pub enum IPCMessage {
    FromRadio(FromRadio),
    ToRadio(ToRadio),
}

#[derive(Debug,Clone,Deserialize)]
pub struct AppConfig {
    pub(crate) metrics_port: u16,
    pub(crate) meshtastic_addr: SocketAddr,
    pub(crate) namespace: String,
    #[serde(skip_deserializing)]
    pub(crate) ___node_id: u32
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            metrics_port: 9941_u16,
            meshtastic_addr: "127.0.0.1:4403".parse().unwrap(),
            namespace: "meshtastic".to_string(),

            ___node_id: 0_u32
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum Connection {
    TCP(String, u16),
    Serial(String),
    #[default]
    None,
}
