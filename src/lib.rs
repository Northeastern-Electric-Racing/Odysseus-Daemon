pub mod mqtt_handler;
pub mod uploader;

// MODULES
pub mod audible;
pub mod can_handler;
pub mod daq;
pub mod daq_monitor;
pub mod lockdown;
pub mod logger;
pub mod numerical;
pub mod sys_parser;
pub mod visual;

// PROTOBUF
pub mod playback_data;
pub mod serverdata;

/// A message to be sent
#[derive(std::fmt::Debug)]
pub struct PublishableMessage {
    pub topic: String,
    pub data: Vec<f32>,
    pub unit: &'static str,
    pub time: u64,
}

/// Indicate a HV transition
#[derive(Clone, Copy)]
pub enum HVTransition {
    TransitionOn(HVOnData),
    TransitionOff,
}
#[derive(Clone, Copy)]
pub struct HVOnData {
    /// Time HV enabled
    pub time_ms: u64,
}

/// the topic to listen for for HV enable, 1 is on 0 is off
pub const HV_EN_TOPIC: &str = "MPU/State/TSMS";

/// the topic to listen for mute enable, 1 is on 0 is off
pub const MUTE_EN_TOPIC: &str = "WHEEL/Buttons/Mute";

/// the topic to listen for when to send video to scylla, 1 means send
pub const SEND_VIDEO_DATA: &str = "Scylla/Video/Send";

/// the topic to listen for when to send logger data to scylla, 1 means send
pub const SEND_LOGGER_DATA: &str = "Scylla/Logger/Send";

/// the topic to listen for when to send serial data to scylla, 1 means send
pub const SEND_SERIAL_DATA: &str = "Scylla/Serial/Send";

///pub const SEND_
/// The save location for all files
pub static SAVE_LOCATION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
