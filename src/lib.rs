pub mod mqtt_handler;

// MODULES
pub mod audible;
pub mod lockdown;
pub mod logger;
pub mod numerical;
pub mod visual;

// PROTOBUF
pub mod playback_data;
pub mod serverdata;

/// A message to be sent
#[derive(std::fmt::Debug)]
pub struct PublishableMessage {
    pub topic: &'static str,
    pub data: Vec<f32>,
    pub unit: &'static str,
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
