mod meshtastic_interaction;
mod structs;
mod consts;
mod metrics;


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
use ::metrics::gauge;
use tokio::task::JoinHandle;
use crate::consts::{GPS_PRECISION_FACTOR, LOOP_PAUSE_MILLISECONDS};
use anyhow::Result;
use meshtastic::protobufs::from_radio::PayloadVariant;
use meshtastic::protobufs::{DeviceMetadata, MeshPacket, MyNodeInfo, NodeInfo, PortNum, Position, User};
use meshtastic::protobufs::mesh_packet::PayloadVariant as mp_variant;
use strum::Display;
use crate::metrics::*;
use std::string::ToString;
use std::time::{SystemTime, UNIX_EPOCH};
use meshtastic::Message;

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
    let (mut fromradio_thread_tx, mut fromradio_thread_rx) =
        mpsc::channel::<IPCMessage>(consts::MPSC_BUFFER_SIZE);
    let (mut toradio_thread_tx, mut toradio_thread_rx) =
        mpsc::channel::<IPCMessage>(consts::MPSC_BUFFER_SIZE);
    let fromradio_tx = fromradio_thread_tx.clone();
    let mut join_handle: JoinHandle<Result<()>> = tokio::task::spawn(async move {
        meshtastic_loop(conn, fromradio_tx, toradio_thread_rx).await
    });
    //endregion

    while SHUTDOWN.get().is_none() {
        if let Ok(packet) = fromradio_thread_rx.try_recv() {
            if let IPCMessage::FromRadio(inbound_packet) = packet {
                if let Some(fromradio_variant) = inbound_packet.payload_variant {
                    match fromradio_variant {
                        PayloadVariant::Packet(mesh_packet) => process_mesh_packet(&mesh_packet).await,
                        PayloadVariant::MyInfo(my_info) => process_my_info(&my_info).await,
                        PayloadVariant::NodeInfo(node_info) => process_node_info(&node_info).await,
                        PayloadVariant::Metadata(metadata) => process_metadata(&metadata).await,
                        _ => {}
                    }
                }
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

        //region sleep to free up processor
        tokio::time::sleep(time::Duration::from_millis(LOOP_PAUSE_MILLISECONDS)).await;
        //endregion
    };
    info!("After a while, crocodile");
}

pub async fn process_my_info(packet: &MyNodeInfo) {
    info!("{:#?}",packet);
    {
        let mut config = SETTINGS.write().await;
        config.___node_id = packet.my_node_num;
    }
}

pub async fn process_metadata(metadata: &DeviceMetadata) {
    info!("{:#?}", metadata);
    let device_id: String;
    {
        let config = SETTINGS.read().await;
        device_id = format!("!{:x}", config.___node_id);
    }
    let labels = vec![
        (consts::LABEL_DEVICE_ID, device_id),
        (consts::LABEL_FW_VERSION, metadata.firmware_version.clone()),
        (consts::LABEL_HW_MODEL, metadata.hw_model().as_str_name().to_string()),
        (consts::LABEL_DEVICE_ROLE, metadata.role().as_str_name().to_string()),
    ];

    gauge!(METRIC_DEVICE_INFO, &labels).set(get_secs() as f64);
}

pub async fn process_node_info(node_info: &NodeInfo) {
    let device_id = format!("!{:x}", node_info.num);
    let labels = vec![
        (consts::LABEL_DEVICE_ID, device_id)
    ];
    gauge!(METRIC_SNR, &labels).set(node_info.snr);
    gauge!(METRIC_HOPS_AWAY, &labels).set(node_info.hops_away);
    gauge!(METRIC_RX_MSG_COUNT, &labels).increment(1);
    gauge!(METRIC_LAST_HEARD_SECS, &labels).set(node_info.last_heard);
    if let Some(dm) = &node_info.device_metrics {
        gauge!(METRIC_CHAN_UTIL, &labels).set(dm.channel_utilization);
        gauge!(METRIC_AIR_UTIL, &labels).set(dm.air_util_tx);
        gauge!(METRIC_BATTERY, &labels).set(dm.battery_level);
        gauge!(METRIC_VOLTAGE, &labels).set(dm.voltage);
        gauge!(METRIC_UPTIME, &labels).set(dm.uptime_seconds);
    }
}

pub async fn process_mesh_packet(mesh_packet: &MeshPacket) {
    if let Some(variant) = mesh_packet.payload_variant.clone() {
        if let mp_variant::Decoded(content) = variant {
            match content.portnum() {
                PortNum::PositionApp => process_position_app(&mesh_packet).await,
                PortNum::NodeinfoApp => process_nodeinfo_app(&mesh_packet).await,
                PortNum::TelemetryApp => {}
                PortNum::NeighborinfoApp => {}
                _ => {
                    info!("Received a payload of {} but we don't consume its data.",content.portnum().as_str_name());
                }
            }
        }
    }
}

pub async fn process_position_app(packet: &MeshPacket) {
    let mp_variant::Decoded(content) = packet.clone().payload_variant.unwrap() else { unreachable!() };
    let data = Position::decode(content.payload.as_slice()).unwrap();
    let coord = geohash::Coord {
        x: GPS_PRECISION_FACTOR.mul(data.latitude_i as f32) as f64,
        y: GPS_PRECISION_FACTOR.mul(data.longitude_i as f32) as f64,
    };
    let device_id = match content.source {
        0 => { format!("!{:x}", packet.from) }
        _ => { format!("!{:x}", content.source) }
    };
    let labels = vec![
        (consts::LABEL_DEVICE_ID, device_id),
        (consts::LABEL_GEOHASH, geohash::encode(coord, 10).unwrap()),
    ];
    gauge!(METRIC_POS_SATS_IN_VIEW, &labels).set(data.sats_in_view);
}

pub async fn process_nodeinfo_app(packet: &MeshPacket) {
    let mp_variant::Decoded(content) = packet.clone().payload_variant.unwrap() else { unreachable!() };
    let data = User::decode(content.payload.as_slice()).unwrap();
    let device_id = match content.source {
        0 => { format!("!{:x}", packet.from) }
        _ => { format!("!{:x}", content.source) }
    };
    let labels = vec![
        (consts::LABEL_DEVICE_ID, data.clone().id),
        (consts::LABEL_HW_MODEL, data.clone().hw_model().as_str_name().to_string()),
        (consts::LABEL_DEVICE_ROLE, data.clone().role().as_str_name().to_string()),
        (consts::LABEL_LICENSED, data.clone().is_licensed.to_string()),
        (consts::LABEL_SHORT_NAME, data.clone().short_name),
        (consts::LABEL_LONG_NAME, data.clone().long_name),
    ];

    gauge!(METRIC_DEVICE_INFO, &labels).set(get_secs() as f64);
}

pub fn get_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
