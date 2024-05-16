use meshtastic::protobufs::{DeviceMetadata, MeshPacket, MyNodeInfo, NodeInfo, PortNum, Position, Telemetry, User};
use meshtastic::protobufs::mesh_packet::PayloadVariant as mp_variant;
use metrics::{counter, gauge};
use meshtastic::protobufs::telemetry::Variant;
use meshtastic::Message;
use std::ops::Mul;
use crate::{consts, SETTINGS, app_metrics};
use crate::consts::{GPS_PRECISION_FACTOR, LABEL_SENSOR_CHANNEL};

pub async fn process_my_info(packet: &MyNodeInfo) {
    info!("Connected to node !{:x}",packet.my_node_num);
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
        (consts::LABEL_DEVICE_ID, device_id.clone()),
        (consts::LABEL_FW_VERSION, metadata.firmware_version.clone()),
        (consts::LABEL_HW_MODEL, metadata.hw_model().as_str_name().to_string()),
        (consts::LABEL_DEVICE_ROLE, metadata.role().as_str_name().to_string()),
    ];
    info!("Received metadata update for {device_id}");
    gauge!(app_metrics::METRIC_DEVICE_INFO, &labels).set(crate::get_secs() as f64);
}

pub async fn process_node_info(node_info: &NodeInfo) {
    let device_id = format!("!{:x}", node_info.num);
    let labels = vec![
        (consts::LABEL_DEVICE_ID, device_id.clone())
    ];
    info!("Received cached NodeInfo for {device_id}");
    gauge!(app_metrics::METRIC_SNR, &labels).set(node_info.snr);
    gauge!(app_metrics::METRIC_HOPS_AWAY, &labels).set(node_info.hops_away);
    counter!(app_metrics::METRIC_RX_MSG_COUNT, &labels).increment(1);
    gauge!(app_metrics::METRIC_LAST_HEARD_SECS, &labels).set(node_info.last_heard);
    if let Some(dm) = &node_info.device_metrics {
        gauge!(app_metrics::METRIC_CHAN_UTIL, &labels).set(dm.channel_utilization);
        gauge!(app_metrics::METRIC_AIR_UTIL, &labels).set(dm.air_util_tx);
        gauge!(app_metrics::METRIC_BATTERY, &labels).set(dm.battery_level);
        gauge!(app_metrics::METRIC_VOLTAGE, &labels).set(dm.voltage);
        gauge!(app_metrics::METRIC_UPTIME, &labels).set(dm.uptime_seconds);
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
                    info!("Received a payload type of {} but we don't consume its data.",content.portnum().as_str_name());
                }
            }
            let device_id = match content.source {
                0 => { format!("!{:x}", mesh_packet.from) }
                _ => { format!("!{:x}", content.source) }
            };
            let labels = vec![
                (consts::LABEL_DEVICE_ID, device_id),
            ];
            counter!(app_metrics::METRIC_RX_MSG_COUNT, &labels).increment(1);
        }
    }
}

pub async fn process_position_app(packet: &MeshPacket) {
    let mp_variant::Decoded(content) = packet.clone().payload_variant.unwrap() else { unreachable!() };
    let data = Position::decode(content.payload.as_slice()).unwrap();
    let coord = geohash::Coord {
        x: GPS_PRECISION_FACTOR.mul(data.longitude_i as f32) as f64,
        y: GPS_PRECISION_FACTOR.mul(data.latitude_i as f32) as f64,
    };
    let device_id = match content.source {
        0 => { format!("!{:x}", packet.from) }
        _ => { format!("!{:x}", content.source) }
    };
    let labels = vec![
        (consts::LABEL_DEVICE_ID, device_id.clone()),
        (consts::LABEL_GEOHASH, geohash::encode(coord, 10).unwrap()),
    ];
    info!("Updating position data for {device_id}");
    gauge!(app_metrics::METRIC_POS_SATS_IN_VIEW, &labels).set(data.sats_in_view);
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

    info!("Received updated NodeInfo data for {}",data.clone().id);
    gauge!(app_metrics::METRIC_DEVICE_INFO, &labels).set(crate::get_secs() as f64);
}

pub async fn process_telemetry_app(packet: &MeshPacket) {
    let mp_variant::Decoded(content) = packet.clone().payload_variant.unwrap() else { unreachable!() };
    let data = Telemetry::decode(content.payload.as_slice()).unwrap();
    let device_id = match content.source {
        0 => { format!("!{:x}", packet.from) }
        _ => { format!("!{:x}", content.source) }
    };
    let mut labels = vec![
        (consts::LABEL_DEVICE_ID, device_id.clone())
    ];
    match data.variant.unwrap() {
        Variant::DeviceMetrics(dm) => {
            info!("Processing DeviceMetrics telemetry for {device_id}");
            gauge!(app_metrics::METRIC_CHAN_UTIL, &labels).set(dm.channel_utilization);
            gauge!(app_metrics::METRIC_AIR_UTIL, &labels).set(dm.air_util_tx);
            gauge!(app_metrics::METRIC_BATTERY, &labels).set(dm.battery_level);
            gauge!(app_metrics::METRIC_VOLTAGE, &labels).set(dm.voltage);
            gauge!(app_metrics::METRIC_UPTIME, &labels).set(dm.uptime_seconds);
        }
        Variant::EnvironmentMetrics(em) => {
            info!("Processing EnvironmentMetrics telemetry for {device_id}");
            gauge!(app_metrics::METRIC_TEMPERATURE, &labels).set(em.temperature);
            gauge!(app_metrics::METRIC_HUMIDITY, &labels).set(em.relative_humidity);
            gauge!(app_metrics::METRIC_BAROMETRIC_PRESSURE, &labels).set(em.barometric_pressure);
            gauge!(app_metrics::METRIC_IAQ, &labels).set(em.iaq);
            gauge!(app_metrics::METRIC_GAS_RESISTANCE, &labels).set(em.gas_resistance);
        }
        Variant::AirQualityMetrics(aq) => {
            info!("Processing AirQualityMetrics telemetry for {device_id}");
            gauge!(app_metrics::METRIC_PARTICLES_03UM, &labels).set(aq.particles_03um);
            gauge!(app_metrics::METRIC_PARTICLES_05UM, &labels).set(aq.particles_05um);
            gauge!(app_metrics::METRIC_PARTICLES_10UM, &labels).set(aq.particles_10um);
            gauge!(app_metrics::METRIC_PARTICLES_25UM, &labels).set(aq.particles_25um);
            gauge!(app_metrics::METRIC_PARTICLES_50UM, &labels).set(aq.particles_50um);
            gauge!(app_metrics::METRIC_PARTICLES_100UM, &labels).set(aq.particles_100um);
            gauge!(app_metrics::METRIC_PM10_STANDARD, &labels).set(aq.pm10_standard);
            gauge!(app_metrics::METRIC_PM25_STANDARD, &labels).set(aq.pm25_standard);
            gauge!(app_metrics::METRIC_PM100_STANDARD, &labels).set(aq.pm100_standard);
            gauge!(app_metrics::METRIC_PM10_ENVIRONMENTAL, &labels).set(aq.pm10_environmental);
            gauge!(app_metrics::METRIC_PM25_ENVIRONMENTAL, &labels).set(aq.pm25_environmental);
            gauge!(app_metrics::METRIC_PM100_ENVIRONMENTAL, &labels).set(aq.pm100_environmental);
        }
        Variant::PowerMetrics(pwr) => {
            info!("Processing PowerMetrics telemetry for {device_id}");
            if pwr.ch1_voltage > 0.0 {
                let mut l_ch1 = labels.clone();
                l_ch1.push((LABEL_SENSOR_CHANNEL, "ch1".to_string()));
                gauge!(app_metrics::METRIC_VOLTAGE, &l_ch1).set(pwr.ch1_voltage);
                gauge!(app_metrics::METRIC_CURRENT, &l_ch1).set(pwr.ch1_current);
            }
            if pwr.ch2_voltage > 0.0 {
                let mut l_ch2 = labels.clone();
                l_ch2.push((LABEL_SENSOR_CHANNEL, "ch2".to_string()));
                gauge!(app_metrics::METRIC_VOLTAGE, &l_ch2).set(pwr.ch2_voltage);
                gauge!(app_metrics::METRIC_CURRENT, &l_ch2).set(pwr.ch2_current);
            }
            if pwr.ch3_voltage > 0.0 {
                let mut l_ch3 = labels.clone();
                l_ch3.push((LABEL_SENSOR_CHANNEL, "ch3".to_string()));
                gauge!(app_metrics::METRIC_VOLTAGE, &l_ch3).set(pwr.ch3_voltage);
                gauge!(app_metrics::METRIC_CURRENT, &l_ch3).set(pwr.ch3_current);
            }
        }
    }
}
