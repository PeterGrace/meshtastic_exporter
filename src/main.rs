mod meshtastic_interaction;
mod structs;
mod consts;
mod app_metrics;
mod processing;


#[macro_use]
extern crate tokio;
#[macro_use]
extern crate tracing;

use std::collections::HashMap;
use crate::meshtastic_interaction::meshtastic_loop;
use std::fs;
use std::net::SocketAddr;
use std::ops::Mul;
use crate::structs::{AppConfig, Connection, IPCMessage};
use std::process;
use lazy_static::lazy_static;
use metrics_exporter_prometheus::PrometheusBuilder;
use serde::Deserialize;
use tokio::sync::{mpsc, OnceCell, RwLock};
use tracing_subscriber::EnvFilter;
use metrics_util::MetricKindMask;
use std::time;
use ::metrics::{counter, gauge};
use tokio::task::JoinHandle;
use crate::consts::{GPS_PRECISION_FACTOR, HEARTBEAT_INTERVAL, LABEL_SENSOR_CHANNEL, LOOP_PAUSE_MILLISECONDS};
use anyhow::Result;
use meshtastic::protobufs::from_radio::PayloadVariant;
use meshtastic::protobufs::{DeviceMetadata, MeshPacket, MyNodeInfo, NodeInfo, PortNum, Position, Telemetry, to_radio, ToRadio, User, Heartbeat};
use meshtastic::protobufs::mesh_packet::PayloadVariant as mp_variant;
use strum::Display;
use crate::app_metrics::*;
use std::string::ToString;
use std::time::{SystemTime, UNIX_EPOCH};
use meshtastic::Message;
use meshtastic::protobufs::telemetry::Variant;

pub fn die(msg: &str) {
    println!("{}", msg);
    process::exit(1);
}



lazy_static! {
      static ref SHUTDOWN: OnceCell<bool> = OnceCell::new();
          //region create SETTINGS static object
      static ref SETTINGS: RwLock<AppConfig> = RwLock::new({
           let cfg_file = match std::env::var("CONFIG_FILE_PATH") {
               Ok(s) => s,
               Err(_e) => { "./config.yaml".to_string()}
           };
          let yaml = fs::read_to_string(cfg_file).unwrap_or_else(|e| {
              die(&format!("Can't read config file: {e}"));
              String::default()
              });
          let gc: AppConfig = match serde_yaml::from_str(&yaml)  {
              Ok(gc) => gc,
              Err(e) => { die(&format!("Couldn't deserialize AppConfig: {e}"));
              AppConfig::default()}
          };
          gc
      });
      //endregion

    // I'm using a static ref here but I don't necessarily need to, yet.
      static ref DEAD_MAN_SWITCH: RwLock<u64> = RwLock::new(0_u64);
  }

#[tokio::main]
pub async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let config: AppConfig;
    {
        config = SETTINGS.read().await.clone();
    }

    let conn = Connection::TCP(config.meshtastic_addr.ip().to_string(), config.meshtastic_addr.port());

    //region create prometheus registry
    let metrics_addr: SocketAddr = format!("0.0.0.0:{}", config.metrics_port).parse().unwrap();
    let prometheus_builder = PrometheusBuilder::new().idle_timeout(
        MetricKindMask::COUNTER | MetricKindMask::HISTOGRAM,
        Some(time::Duration::from_secs(10)),
    )
        .with_http_listener(metrics_addr)
        .install().expect("Couldn't start prometheus.");
    register_metrics();
    //endregion

    //region spawn meshtastic connection thread
    let (fromradio_thread_tx, mut fromradio_thread_rx) =
        mpsc::channel::<IPCMessage>(consts::MPSC_BUFFER_SIZE);
    let (toradio_thread_tx, toradio_thread_rx) =
        mpsc::channel::<IPCMessage>(consts::MPSC_BUFFER_SIZE);
    let fromradio_tx = fromradio_thread_tx.clone();
    let join_handle: JoinHandle<Result<()>> = tokio::task::spawn(async move {
        meshtastic_loop(conn, fromradio_tx, toradio_thread_rx).await
    });
    //endregion

    //initialize deadman with current time
    update_deadman().await;

    let mut last_heartbeat :u64 = get_secs();

    while SHUTDOWN.get().is_none() {
        if let Ok(packet) = fromradio_thread_rx.try_recv() {
            update_deadman().await;
            if let IPCMessage::FromRadio(inbound_packet) = packet {
                if let Some(fromradio_variant) = inbound_packet.payload_variant {
                    match fromradio_variant {
                        PayloadVariant::Packet(mesh_packet) => processing::process_mesh_packet(&mesh_packet).await,
                        PayloadVariant::MyInfo(my_info) => processing::process_my_info(&my_info).await,
                        PayloadVariant::NodeInfo(node_info) => processing::process_node_info(&node_info).await,
                        PayloadVariant::Metadata(metadata) => processing::process_metadata(&metadata).await,
                        _ => {}
                    }
                }
            }
        }

        let heartbeat_diff = get_secs().saturating_sub(last_heartbeat);
        if heartbeat_diff > HEARTBEAT_INTERVAL {
            let tr = ToRadio {
                payload_variant: Some(to_radio::PayloadVariant::Heartbeat(Heartbeat::default()))
            };
            if let Err(e) = toradio_thread_tx.send(IPCMessage::ToRadio(tr)).await {
                error!("Could not send heartbeat packet.  Exiting.");
                SHUTDOWN.set(true).expect("Couldn't set shutdown, so, wanted to shutdown anyway")
            } else {
                last_heartbeat = get_secs();
            }
        }

        //region thread tending
        if join_handle.is_finished() {
            let result = join_handle.await;
            match result {
                Ok(o) => {
                    match o {
                        Ok(_) => {
                            error!("Meshtastic thread exited gracefully.  Shouldn't happen");
                        }
                        Err(e) => {
                            error!("Exiting, meshtastic error: {e}");
                        }
                    }
                }
                Err(e) => {
                    error!("We lost connection to meshtastic device, and we don't know why");
                }
            }
            process::exit(2);
        }
        //endregion

        check_deadman().await;

        //region sleep to free up processor
        tokio::time::sleep(time::Duration::from_millis(LOOP_PAUSE_MILLISECONDS)).await;
        //endregion
    };
    info!("After a while, crocodile");
}


pub fn get_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub async fn update_deadman() {
    let mut dms = DEAD_MAN_SWITCH.write().await;
    *dms = get_secs();
}

pub async fn check_deadman() {
    let dms = DEAD_MAN_SWITCH.read().await;
    let now = get_secs();
    let diff = now.saturating_sub(*dms);
    if diff > consts::DEADMAN_TIMEOUT {
        error!("Deadman switch timer elapsed, we haven't received a packet in {diff} seconds. closing app.");
        SHUTDOWN.set(true).expect("Couldn't set shutdown so.. panic it is");
    }
}
