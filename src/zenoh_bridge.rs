use std::path::PathBuf;

use protobuf::Message;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::{PublishableMessage, playback_data::PlaybackData, serverdata};
use tracing::{trace, warn};

use zenoh::{
    Config,
    bytes::{Encoding, ZBytes},
};

/// Zenoh --> mqtt
pub async fn zenoh_rev(
    cancel_token: CancellationToken,
    conf_path: PathBuf,
    mqtt_send_tx: Sender<PublishableMessage>,
) {
    zenoh::init_log_from_env_or("info");

    let session = zenoh::open(Config::from_file(conf_path).expect("Could not find Zenoh conf"))
        .await
        .expect("Invalid zenoh conf");

    let subscriber = session
        .declare_subscriber("**")
        .await
        .expect("Could not get subscriber");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                break;
            },
            Ok(res) = subscriber.recv_async() => {
                let Some(mqtt) = convert_to_mqtt(res) else {
                    trace!("Got unparsable zenoh message");
                    continue;
                };
                if let Err(e) = mqtt_send_tx.send(mqtt).await {
                    warn!("Error sending message from zenoh to mqtt: {}", e);
                }
            }
        }
    }
}

fn convert_to_mqtt(sample: zenoh::sample::Sample) -> Option<PublishableMessage> {
    let res = serverdata::ServerData::parse_from_reader(&mut sample.payload().reader()).ok()?;

    let topic = sample.key_expr().to_string().replace("|", "?");
    Some(PublishableMessage {
        topic,
        data: res.values,
        unit: res.unit,
        time: res.time_us,
    })
}

/// MQTT --> zenoh
pub async fn zenoh_fwd(
    cancel_token: CancellationToken,
    conf_path: PathBuf,
    mut mqtt_recv_rx: Receiver<PlaybackData>,
) {
    zenoh::init_log_from_env_or("info");

    let session = zenoh::open(Config::from_file(conf_path).expect("Could not find Zenoh conf"))
        .await
        .expect("Invalid zenoh conf");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                break;
            },
            Ok(res) = mqtt_recv_rx.recv() => {
                let (data, ref mut topic) = convert_to_zenoh(res);
                *topic = topic.replace("?", "|");
                trace!("PUTTING {}", topic);
                if let Err(err)= session.put(topic, data).encoding(Encoding::APPLICATION_PROTOBUF).await {
                    warn!("Error sending zenoh message: {}", err);
                }
            }
        }
    }
}

fn convert_to_zenoh(msg: PlaybackData) -> (ZBytes, String) {
    let mut sendable = serverdata::ServerData::new();
    sendable.unit = msg.unit;
    sendable.time_us = msg.time_us;
    sendable.values = msg.values;

    let bytes = ZBytes::from(protobuf::Message::write_to_bytes(&sendable).unwrap());

    (bytes, msg.topic)
}
