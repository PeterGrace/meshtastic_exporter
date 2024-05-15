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
use ::metrics::{counter, gauge};
use tokio::task::JoinHandle;
use crate::consts::{GPS_PRECISION_FACTOR, LABEL_SENSOR_CHANNEL, LOOP_PAUSE_MILLISECONDS};
use anyhow::Result;
use meshtastic::protobufs::from_radio::PayloadVariant;
use meshtastic::protobufs::{DeviceMetadata, MeshPacket, MyNodeInfo, NodeInfo, PortNum, Position, Telemetry, User};
use meshtastic::protobufs::mesh_packet::PayloadVariant as mp_variant;
use strum::Display;
use crate::metrics::*;
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
    counter!(METRIC_RX_MSG_COUNT, &labels).increment(1);
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
                PortNum::TelemetryApp => process_telemetry_app(&mesh_packet).await,
                PortNum::NeighborinfoApp => {}
                _ => {
                    info!("Received a payload of {} but we don't consume its data.",content.portnum().as_str_name());
                }
            }
            let device_id = match content.source {
                0 => { format!("!{:x}", mesh_packet.from) }
                _ => { format!("!{:x}", content.source) }
            };
            let labels = vec![
                (consts::LABEL_DEVICE_ID, device_id),
            ];
            counter!(METRIC_RX_MSG_COUNT, &labels).increment(1);
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

pub async fn process_telemetry_app(packet: &MeshPacket) {
    let mp_variant::Decoded(content) = packet.clone().payload_variant.unwrap() else { unreachable!() };
    let data = Telemetry::decode(content.payload.as_slice()).unwrap();
    let device_id = match content.source {
        0 => { format!("!{:x}", packet.from) }
        _ => { format!("!{:x}", content.source) }
    };
    let mut labels = vec![
        (consts::LABEL_DEVICE_ID, device_id)
    ];
    match data.variant.unwrap() {
        Variant::DeviceMetrics(dm) => {
            gauge!(METRIC_CHAN_UTIL, &labels).set(dm.channel_utilization);
            gauge!(METRIC_AIR_UTIL, &labels).set(dm.air_util_tx);
            gauge!(METRIC_BATTERY, &labels).set(dm.battery_level);
            gauge!(METRIC_VOLTAGE, &labels).set(dm.voltage);
            gauge!(METRIC_UPTIME, &labels).set(dm.uptime_seconds);
        }
        Variant::EnvironmentMetrics(em) => {
            gauge!(METRIC_TEMPERATURE, &labels).set(em.temperature);
            gauge!(METRIC_HUMIDITY, &labels).set(em.relative_humidity);
            gauge!(METRIC_BAROMETRIC_PRESSURE, &labels).set(em.barometric_pressure);
            gauge!(METRIC_IAQ, &labels).set(em.iaq);
            gauge!(METRIC_GAS_RESISTANCE, &labels).set(em.gas_resistance);
        }
        Variant::AirQualityMetrics(aq) => {
            gauge!(METRIC_PARTICLES_03UM, &labels).set(aq.particles_03um);
            gauge!(METRIC_PARTICLES_05UM, &labels).set(aq.particles_05um);
            gauge!(METRIC_PARTICLES_10UM, &labels).set(aq.particles_10um);
            gauge!(METRIC_PARTICLES_25UM, &labels).set(aq.particles_25um);
            gauge!(METRIC_PARTICLES_50UM, &labels).set(aq.particles_50um);
            gauge!(METRIC_PARTICLES_100UM, &labels).set(aq.particles_100um);
            gauge!(METRIC_PM10_STANDARD, &labels).set(aq.pm10_standard);
            gauge!(METRIC_PM25_STANDARD, &labels).set(aq.pm25_standard);
            gauge!(METRIC_PM100_STANDARD, &labels).set(aq.pm100_standard);
            gauge!(METRIC_PM10_ENVIRONMENTAL, &labels).set(aq.pm10_environmental);
            gauge!(METRIC_PM25_ENVIRONMENTAL, &labels).set(aq.pm25_environmental);
            gauge!(METRIC_PM100_ENVIRONMENTAL, &labels).set(aq.pm100_environmental);
        }
        Variant::PowerMetrics(pwr) => {
            if pwr.ch1_voltage > 0.0 {
                let mut l_ch1 = labels.clone();
                l_ch1.push((LABEL_SENSOR_CHANNEL, "ch1".to_string()));
                gauge!(METRIC_VOLTAGE, &l_ch1).set(pwr.ch1_voltage);
                gauge!(METRIC_CURRENT, &l_ch1).set(pwr.ch1_current);
            }
            if pwr.ch2_voltage > 0.0 {
                let mut l_ch2 = labels.clone();
                l_ch2.push((LABEL_SENSOR_CHANNEL, "ch2".to_string()));
                gauge!(METRIC_VOLTAGE, &l_ch2).set(pwr.ch2_voltage);
                gauge!(METRIC_CURRENT, &l_ch2).set(pwr.ch2_current);
            }
            if pwr.ch3_voltage > 0.0 {
                let mut l_ch3 = labels.clone();
                l_ch3.push((LABEL_SENSOR_CHANNEL, "ch3".to_string()));
                gauge!(METRIC_VOLTAGE, &l_ch3).set(pwr.ch3_voltage);
                gauge!(METRIC_CURRENT, &l_ch3).set(pwr.ch3_current);
            }
        }
    }
}


pub fn get_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
