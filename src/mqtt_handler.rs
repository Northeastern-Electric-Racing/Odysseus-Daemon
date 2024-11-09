use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use protobuf::Message;
use rumqttc::v5::{
    mqttbytes::{v5::Packet, QoS},
    AsyncClient, Event, EventLoop, MqttOptions,
};
use tokio::sync::{mpsc::Receiver, watch::Sender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::{serverdata, PublishableMessage, HV_EN_TOPIC, MUTE_EN_TOPIC};

/// The chief processor of incoming mqtt data, this handles
/// - mqtt state
/// - reception via mqtt and subsequent parsing
///
pub struct MqttProcessor {
    cancel_token: CancellationToken,
    mqtt_sender_rx: Receiver<PublishableMessage>,
    hv_stat_send: Sender<bool>,
    mute_stat_send: Sender<bool>,
}

/// processor options, these are static immutable settings
pub struct MqttProcessorOptions {
    /// URI of the mqtt server
    pub mqtt_path: String,
}

impl MqttProcessor {
    /// Creates a new mqtt receiver and sender
    pub fn new(
        cancel_token: CancellationToken,
        mqtt_sender_rx: Receiver<PublishableMessage>,
        hv_stat_send: Sender<bool>,
        mute_stat_send: Sender<bool>,
        opts: MqttProcessorOptions,
    ) -> (MqttProcessor, MqttOptions) {
        // create the mqtt client and configure it
        let mut mqtt_opts = MqttOptions::new(
            format!(
                "Ody-{:?}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_millis()
            ),
            opts.mqtt_path.split_once(':').expect("Invalid Siren URL").0,
            opts.mqtt_path
                .split_once(':')
                .unwrap()
                .1
                .parse::<u16>()
                .expect("Invalid Siren port"),
        );
        mqtt_opts
            .set_keep_alive(Duration::from_secs(20))
            .set_clean_start(false)
            .set_connection_timeout(3)
            //       .set_session_expiry_interval(Some(u32::MAX))
            .set_topic_alias_max(Some(600));

        (
            MqttProcessor {
                cancel_token,
                mqtt_sender_rx,
                hv_stat_send,
                mute_stat_send,
            },
            mqtt_opts,
        )
    }

    /// This handles the reception of mqtt messages, will not return
    /// * `eventloop` - The eventloop returned by ::new to connect to.  The loop isnt sync so this is the best that can be done
    /// * `client` - The async mqttt v5 client to use for subscriptions
    pub async fn process_mqtt(mut self, client: Arc<AsyncClient>, mut eventloop: EventLoop) {
        debug!("Subscribing to siren with inputted topic");
        client
            .subscribe(HV_EN_TOPIC, rumqttc::v5::mqttbytes::QoS::ExactlyOnce)
            .await
            .expect("Could not subscribe to Siren");

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    debug!("Shutting down MQTT processor!");
                    break;
                },
                msg = eventloop.poll() => match msg {
                    Ok(Event::Incoming(Packet::Publish(msg))) => {
                        let Ok(res) = serverdata::ServerData::parse_from_bytes(&msg.payload) else {
                            warn!("Recieved unparsable mqtt message.");
                            continue;
                        };
                        let Ok(topic) = std::str::from_utf8(&msg.topic) else {
                            warn!("Could not parse topic, topic: {:?}", msg.topic);
                            continue;
                        };
                        let val = *res.values.first().unwrap_or(&-1f32) as u8;
                        match topic {
                            HV_EN_TOPIC => {
                                if val == 1 {
                                    self.hv_stat_send.send(true).expect("HV Stat Channel Closed");
                                } else if val == 0 {
                                    self.hv_stat_send.send(false).expect("HV Stat Channel Closed");
                                } else {
                                    warn!("Received bad HV message!");
                                }
                            },
                            MUTE_EN_TOPIC => {
                                if val == 1 {
                                    self.mute_stat_send.send(true).expect("Mute Stat Channel Closed");
                                } else if val == 0 {
                                    self.mute_stat_send.send(false).expect("Mute Stat Channel Closed");
                                } else {
                                    warn!("Received bad mute message!");
                                }
                            },
                            _ => {
                                warn!("Unknown topic received: {}", topic);
                            }
                        }
                    }
                    Err(e) => trace!("Recieved error: {}", e),
                    _ => {}
                },
                sendable = self.mqtt_sender_rx.recv() => {
                    match sendable {
                        Some(sendable) => {
                            trace!("Sending {:?}", sendable);
                            let mut payload = serverdata::ServerData::new();
                            payload.unit = sendable.unit.to_string();
                            payload.values = sendable.data;
                            payload.time_us =  SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .expect("Time went backwards").as_micros() as u64;
                            let Ok(bytes) = protobuf::Message::write_to_bytes(&payload) else {
                                warn!("Failed to serialize protobuf message!");
                                continue;
                            };
                            let Ok(_) = client.publish(sendable.topic, QoS::ExactlyOnce, false, bytes).await else {
                                warn!("Failed to send MQTT message!");
                                continue;
                            };
                        },
                        None => continue,
                    }
                }
            }
        }
    }
}
