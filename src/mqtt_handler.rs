use odysseus_uploader::upload_files;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use protobuf::{Message, SpecialFields};
use rumqttc::v5::{
    mqttbytes::{v5::Packet, QoS},
    AsyncClient, Event, EventLoop, MqttOptions,
};
use tokio::sync::{
    mpsc::{self, Receiver},
    watch::Sender,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::{
    playback_data, serverdata, HVTransition, PublishableMessage, HV_EN_TOPIC, MUTE_EN_TOPIC,
    SAVE_LOCATION, SEND_LOGGER_DATA, SEND_SERIAL_DATA, SEND_VIDEO_DATA,
};

/// The chief processor of incoming mqtt data, this handles
/// - mqtt state
/// - reception via mqtt and subsequent parsing
///     Takes in many channels:
/// - mqtt_sender_rx: A receiver of any messages, it then publishes them
/// - hv_stat_send: A sender of the current HV state (only if it changes!), will be set to ON if augment_hv_on is true
/// - mute_stat_send: A sender of the current mute button state
/// - mqtt_recv_tx: Optional, a sender of all mqtt messages, if None no messages sent
pub struct MqttProcessor {
    cancel_token: CancellationToken,
    mqtt_sender_rx: Receiver<PublishableMessage>,
    hv_stat_send: Sender<HVTransition>,
    augment_hv_on: bool,
    mute_stat_send: Sender<bool>,
    mqtt_recv_tx: Option<mpsc::Sender<playback_data::PlaybackData>>,
    opts: MqttProcessorOptions,
}

/// processor options, these are static immutable settings
pub struct MqttProcessorOptions {
    /// URI of the mqtt server
    pub mqtt_path: String,
    /// URI of scylla
    pub scylla_url: String,
    /// The output path for the folder that contains the video, logger, and serial files
    pub output_folder: String,
}

impl MqttProcessor {
    /// Creates a new mqtt receiver and sender
    pub fn new(
        cancel_token: CancellationToken,
        mqtt_sender_rx: Receiver<PublishableMessage>,
        hv_stat_send: Sender<HVTransition>,
        augment_hv_on: bool,
        mute_stat_send: Sender<bool>,
        mqtt_recv_tx: Option<mpsc::Sender<playback_data::PlaybackData>>,
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
                augment_hv_on,
                mute_stat_send,
                mqtt_recv_tx,
                opts,
            },
            mqtt_opts,
        )
    }

    /// This handles the reception of mqtt messages, will not return
    /// * `eventloop` - The eventloop returned by ::new to connect to.  The loop isnt sync so this is the best that can be done
    /// * `client` - The async mqttt v5 client to use for subscriptions
    pub async fn process_mqtt(mut self, client: Arc<AsyncClient>, mut eventloop: EventLoop) {
        debug!("Subscribing to siren, all topics");
        client
            .subscribe("#", rumqttc::v5::mqttbytes::QoS::ExactlyOnce)
            .await
            .expect("Could not subscribe to Siren");

        // if augment HV on, send as such, otherwise start default off
        let mut last_stat = if self.augment_hv_on {
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            warn!("HV status permanently set on!!");
            if let Err(err) =
                std::fs::create_dir(format!("{}/event-{}", SAVE_LOCATION.get().unwrap(), time))
            {
                panic!(
                    "Could not create folder for data, bailing out of this loop! {}",
                    err
                );
            }
            self.hv_stat_send
                .send(HVTransition::TransitionOn(crate::HVOnData {
                    time_ms: time,
                }))
                .expect("HV Stat Channel Closed");
            true
        } else {
            false
        };

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
                                if !self.augment_hv_on {
                                    // ensure only triggering upon change from previous loop
                                 if val == 1 && !last_stat {
                                        debug!("Transitioning states to HV on, creating folder!");
                                        if let Err(err) = std::fs::create_dir(format!("{}/event-{}", SAVE_LOCATION.get().unwrap(), res.time_us / 1000)) {
                                            warn!("Could not create folder for data, bailing out of this loop! {}", err);
                                            continue;
                                        }
                                       self.hv_stat_send.send(HVTransition::TransitionOn(
                                           crate::HVOnData { time_ms:  res.time_us / 1000})).expect("HV Stat Channel Closed");
                                        last_stat = true;
                                    } else if val == 0 && last_stat {
                                        debug!("Transitioning states to HV off");
                                       self.hv_stat_send.send(HVTransition::TransitionOff).expect("HV Stat Channel Closed");
                                       last_stat = false;
                                    } else if val != 0 && val != 1 {
                                        warn!("Received bad HV message!");
                                    }
                                }
                            },
                            MUTE_EN_TOPIC => {
                                // mute button messages should be single shot
                                if val == 1 {
                                    self.mute_stat_send.send(true).expect("Mute Stat Channel Closed");
                                } else if val == 0 {
                                    self.mute_stat_send.send(false).expect("Mute Stat Channel Closed");
                                } else {
                                    warn!("Received bad mute message!");
                                }
                            },
                            SEND_LOGGER_DATA => {
                                println!("Sending Logger Data, {}", val);

                                if val == 1{
                                    upload_files(&self.opts.output_folder, &self.opts.scylla_url, true, false, false);
                                }
                            },
                            SEND_SERIAL_DATA => {
                                println!("Sending Serial Data, {}", val);

                                if val == 1{
                                    upload_files(&self.opts.output_folder, &self.opts.scylla_url, false, false, true);
                                }
                            },
                            SEND_VIDEO_DATA => {
                                println!("Sending Video Data, {}", val);

                                if val == 1{
                                    upload_files(&self.opts.output_folder, &self.opts.scylla_url, false, true, false);
                                }
                            }
                            _ => {
                            }
                        }
                        // if using it, send all mqtt messages to data logger
                        if let Some(ref recv) = self.mqtt_recv_tx {
                            if let Err(err) = recv.send(playback_data::PlaybackData{
                                topic:topic.to_string(),values:res.values,unit:res.unit,time_us:res.time_us, special_fields: SpecialFields::new() }).await {
                                warn!("Error sending message received! {}", err);
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
