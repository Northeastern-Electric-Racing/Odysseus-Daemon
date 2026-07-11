//! HELPER: Receive and send MQTT

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use protobuf::{Message, SpecialFields};
use tokio::sync::{broadcast, mpsc::Receiver, watch::Sender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};
use zenoh::{Config, Session, bytes::Encoding, sample::Sample};

use crate::{
    HV_EN_TOPIC, HVTransition, MUTE_EN_TOPIC, PublishableMessage, SAVE_LOCATION, SEND_LOGGER_DATA,
    SEND_SERIAL_DATA, SEND_VIDEO_DATA, playback_data, serverdata, uploader::upload_files,
};

/// The chief processor of incoming zenoh data, this handles
/// - zenoh state
/// - reception via mqtt and subsequent parsing
///   Takes in many channels:
/// - zenoh_sender_rx: A receiver of any messages, it then publishes them
/// - hv_stat_send: A sender of the current HV state (only if it changes!), will be set to ON if augment_hv_on is true
/// - mute_stat_send: A sender of the current mute button state
/// - zenoh_recv_tx: Optional, a sender of all zenoh messages, if None no messages sent
pub struct ZenohProcessor {
    cancel_token: CancellationToken,
    zenoh_sender_rx: Receiver<PublishableMessage>,
    hv_stat_send: Sender<HVTransition>,
    augment_hv_on: bool,
    mute_stat_send: Sender<bool>,
    zenoh_recv_tx: Option<broadcast::Sender<playback_data::PlaybackData>>,
    scylla_url: Option<String>,
    session: Session,
}

#[allow(clippy::too_many_arguments)]
impl ZenohProcessor {
    /// Creates a new mqtt receiver and sender
    pub async fn new(
        cancel_token: CancellationToken,
        mqtt_sender_rx: Receiver<PublishableMessage>,
        hv_stat_send: Sender<HVTransition>,
        augment_hv_on: bool,
        mute_stat_send: Sender<bool>,
        mqtt_recv_tx: Option<broadcast::Sender<playback_data::PlaybackData>>,
        conf_path: PathBuf,
        scylla_url: Option<String>,
    ) -> ZenohProcessor {
        zenoh::init_log_from_env_or("info");

        let session = zenoh::open(Config::from_file(conf_path).expect("Could not find Zenoh conf"))
            .await
            .expect("Invalid zenoh conf");

        ZenohProcessor {
            cancel_token,
            zenoh_sender_rx: mqtt_sender_rx,
            hv_stat_send,
            augment_hv_on,
            mute_stat_send,
            zenoh_recv_tx: mqtt_recv_tx,
            scylla_url,
            session,
        }
    }

    fn convert_to_mqtt(sample: zenoh::sample::Sample) -> Option<PublishableMessage> {
        let res = serverdata::ServerData::parse_from_reader(&mut sample.payload().reader()).ok()?;

        Some(PublishableMessage {
            topic: sample.key_expr().to_string(),
            data: res.values,
            unit: res.unit,
            time: res.time_us,
        })
    }

    async fn handle_recv(&self, sample: Sample, last_stat: &mut bool) {
        let Some(msg) = Self::convert_to_mqtt(sample) else {
            warn!("Could not deserialize Zenoh incoming!");
            return;
        };
        let val = *msg.data.first().unwrap_or(&-1f32) as u8;
        match msg.topic.as_str() {
            HV_EN_TOPIC => {
                if !self.augment_hv_on {
                    // ensure only triggering upon change from previous loop
                    if val == 1 && !*last_stat {
                        debug!("Transitioning states to HV on, creating folder!");
                        if let Err(err) = std::fs::create_dir(format!(
                            "{}/event-{}",
                            SAVE_LOCATION.get().unwrap(),
                            msg.time / 1000
                        )) {
                            warn!(
                                "Could not create folder for data, bailing out of this loop! {}",
                                err
                            );
                            return;
                        }
                        self.hv_stat_send
                            .send(HVTransition::TransitionOn(crate::HVOnData {
                                time_ms: msg.time / 1000,
                            }))
                            .expect("HV Stat Channel Closed");
                        *last_stat = true;
                    } else if val == 0 && *last_stat {
                        debug!("Transitioning states to HV off");
                        self.hv_stat_send
                            .send(HVTransition::TransitionOff)
                            .expect("HV Stat Channel Closed");
                        *last_stat = false;
                    } else if val != 0 && val != 1 {
                        warn!("Received bad HV message!");
                    }
                }
            }
            MUTE_EN_TOPIC => {
                // mute button messages should be single shot
                if val == 1 {
                    self.mute_stat_send
                        .send(true)
                        .expect("Mute Stat Channel Closed");
                } else if val == 0 {
                    self.mute_stat_send
                        .send(false)
                        .expect("Mute Stat Channel Closed");
                } else {
                    warn!("Received bad mute message!");
                }
            }
            SEND_LOGGER_DATA => {
                if !*last_stat && let Some(url) = &self.scylla_url {
                    info!("Sending Logger Data, {}", val);

                    upload_files(SAVE_LOCATION.get().unwrap(), url, true, false, false);
                }
            }
            SEND_SERIAL_DATA => {
                if !*last_stat && let Some(url) = &self.scylla_url {
                    info!("Sending Serial Data, {}", val);

                    upload_files(SAVE_LOCATION.get().unwrap(), url, false, false, true);
                }
            }
            SEND_VIDEO_DATA => {
                if !*last_stat && let Some(url) = &self.scylla_url {
                    info!("Sending Video Data, {}", val);

                    upload_files(SAVE_LOCATION.get().unwrap(), url, false, true, false);
                }
            }
            _ => {}
        }
        // if using it, send all mqtt messages to data logger
        if let Some(ref recv) = self.zenoh_recv_tx
            && let Err(err) = recv.send(playback_data::PlaybackData {
                topic: msg.topic.to_string(),
                values: msg.data,
                unit: msg.unit,
                time_us: msg.time,
                special_fields: SpecialFields::new(),
            })
        {
            warn!("Error sending message received! {}", err);
        }
    }

    /// This handles the reception of mqtt messages, will not return
    pub async fn process_zenoh(mut self) {
        debug!("Subscribing to siren, all topics");
        let subscriber = self
            .session
            .declare_subscriber("**")
            .await
            .expect("Could not subscribe to MQTT");

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
                panic!("Could not create folder for data, bailing out of this loop! {err}");
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
                    debug!("Shutting down Zenoh processor!");
                    break;
                },
                Ok(msg) = subscriber.recv_async() => {
                        self.handle_recv(msg, &mut last_stat).await;
                },
                Some(sendable) = self.zenoh_sender_rx.recv() => {
                    trace!("Sending {:?}", sendable);
                    let mut payload = serverdata::ServerData::new();
                    payload.unit = sendable.unit.to_string();
                    payload.values = sendable.data;
                    payload.time_us =  sendable.time;
                    let Ok(bytes) = protobuf::Message::write_to_bytes(&payload) else {
                        warn!("Failed to serialize protobuf message!");
                        continue;
                    };

                    if let Err(err)= self.session.put(sendable.topic, bytes).encoding(Encoding::APPLICATION_PROTOBUF).await {
                        warn!("Error sending zenoh message: {}", err);
                    }
                }
            }
        }
    }
}
