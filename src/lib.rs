pub mod lockdown;
pub mod mqtt_handler;
pub mod numerical;
pub mod serverdata;
pub mod visual;

#[derive(std::fmt::Debug)]
pub struct PublishableMessage {
    pub topic: &'static str,
    pub data: Vec<f32>,
    pub unit: &'static str,
}

/// the topic to listen for for HV enable, 1 is on 0 is off
pub const HV_EN_TOPIC: &str = "MPU/State/TSMS";
