use metrics::{describe_counter, describe_gauge, describe_histogram};

pub const METRIC_DEVICE_INFO: &str = "meshtastic_device_info";
pub const METRIC_HOPS_AWAY: &str = "meshtastic_hops_away";
pub const METRIC_TEMPERATURE: &str = "meshtastic_temperature";

pub const METRIC_RSSI: &str = "meshtastic_rssi";
pub const METRIC_SNR: &str = "meshtastic_snr";
pub const METRIC_VOLTAGE: &str = "meshtastic_voltage";
pub const METRIC_CURRENT: &str = "meshtastic_current";
pub const METRIC_BATTERY: &str = "meshtastic_battery";
pub const METRIC_HUMIDITY: &str = "meshtastic_humidity";
pub const METRIC_BAROMETRIC_PRESSURE: &str = "meshtastic_barometric_pressure";
pub const METRIC_RX_MSG_COUNT: &str = "meshtastic_received_message_count";
pub const METRIC_LAST_HEARD_SECS: &str = "meshtastic_last_heard_seconds";
pub const METRIC_CHAN_UTIL: &str = "meshtastic_channel_utilization";
pub const METRIC_AIR_UTIL: &str = "meshtastic_air_utilization";
pub const METRIC_UPTIME: &str = "meshtastic_device_uptime_seconds";

pub const METRIC_POS_SATS_IN_VIEW: &str = "meshtastic_satellites_in_view";
pub const METRIC_IAQ: &str = "meshtastic_indoor_air_quality";
pub const METRIC_GAS_RESISTANCE: &str = "meshtastic_gas_resistance";

pub const METRIC_PM10_STANDARD: &str = "meshtastic_air_pm10_standard";
pub const METRIC_PM25_STANDARD: &str = "meshtastic_air_pm25_standard";
pub const METRIC_PM100_STANDARD: &str = "meshtastic_air_pm100_standard";
pub const METRIC_PM10_ENVIRONMENTAL: &str = "meshtastic_air_pm10_environmental";
pub const METRIC_PM25_ENVIRONMENTAL: &str = "meshtastic_air_pm25_environmental";
pub const METRIC_PM100_ENVIRONMENTAL: &str = "meshtastic_air_pm100_environmental";
pub const METRIC_PARTICLES_03UM: &str = "meshtastic_air_particles_03um";
pub const METRIC_PARTICLES_05UM: &str = "meshtastic_air_particles_05um";
pub const METRIC_PARTICLES_10UM: &str = "meshtastic_air_particles_10um";
pub const METRIC_PARTICLES_25UM: &str = "meshtastic_air_particles_25um";
pub const METRIC_PARTICLES_50UM: &str = "meshtastic_air_particles_50um";
pub const METRIC_PARTICLES_100UM: &str = "meshtastic_air_particles_100um";

pub fn register_metrics() {
    describe_gauge!(METRIC_DEVICE_INFO, "information about device");
    describe_gauge!(METRIC_HOPS_AWAY, "The reported number of hops away the client is.");

    describe_gauge!(METRIC_TEMPERATURE,"The reported temperature from telemetry module");
    describe_gauge!(METRIC_HUMIDITY, "The relative humidity from telemetry module");
    describe_gauge!(METRIC_BAROMETRIC_PRESSURE, "the barometric pressure reported from telemetry module");

    describe_gauge!(METRIC_VOLTAGE, "The reported device voltage");
    describe_gauge!(METRIC_CURRENT, "The reported device current");
    describe_gauge!(METRIC_BATTERY, "The reported device battery remaining");

    describe_gauge!(METRIC_SNR, "Signal to noise ratio for node");
    describe_gauge!(METRIC_RSSI, "RSSI for node");

    describe_counter!(METRIC_RX_MSG_COUNT, "The number of messages received from the node");
    describe_gauge!(METRIC_LAST_HEARD_SECS, "The number of seconds last heard");

    describe_gauge!(METRIC_CHAN_UTIL,"The channel's utilization");
    describe_gauge!(METRIC_AIR_UTIL, "The amount of total utilization");

    describe_gauge!(METRIC_UPTIME, "The total seconds the device has been energized.");

    describe_gauge!(METRIC_POS_SATS_IN_VIEW, "The number of satellites in view for this node.");
    describe_gauge!(METRIC_IAQ, "relative scale of VOC content measured from 0-500");
    describe_gauge!(METRIC_GAS_RESISTANCE, "Gas resistance in MOhms");

    describe_gauge!(METRIC_PM10_STANDARD, "not sure how to describe this");
    describe_gauge!(METRIC_PM25_STANDARD, "not sure how to describe this");
    describe_gauge!(METRIC_PM100_STANDARD, "not sure how to describe this");
    describe_gauge!(METRIC_PM10_ENVIRONMENTAL, "not sure how to describe this");
    describe_gauge!(METRIC_PM25_ENVIRONMENTAL, "not sure how to describe this");
    describe_gauge!(METRIC_PM100_ENVIRONMENTAL, "not sure how to describe this");

    describe_gauge!(METRIC_PARTICLES_03UM, "not sure how to describe this");
    describe_gauge!(METRIC_PARTICLES_05UM, "not sure how to describe this");
    describe_gauge!(METRIC_PARTICLES_10UM, "not sure how to describe this");
    describe_gauge!(METRIC_PARTICLES_25UM, "not sure how to describe this");
    describe_gauge!(METRIC_PARTICLES_50UM, "not sure how to describe this");
    describe_gauge!(METRIC_PARTICLES_100UM, "not sure how to describe this");

}
