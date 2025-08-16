use std::time::UNIX_EPOCH;

use regex::Regex;
use rumqttc::v5::mqttbytes::v5::Publish;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::PublishableMessage;

/// Parse the SYS module.  Requires all SYS messages.
pub async fn sys_parser(
    cancel_token: CancellationToken,
    mut mqtt_recv_rx: Receiver<Publish>,
    mqtt_send_tx: Sender<PublishableMessage>,
) {
    let num_regex = Regex::new(r"\d+(\.\d+)?").unwrap();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                break;
            },
            Some(msg) = mqtt_recv_rx.recv() => {
                let Ok(topic) = String::from_utf8(msg.topic.to_vec()) else {
                            warn!("Could not parse topic, topic: {:?}", msg.topic);
                            continue;
                };
                let data = match std::str::from_utf8(&msg.payload) {
                    Ok(data) => data.to_string(),
                    Err(err) => {
                        warn!("Could not parse payload manually, topic: {:?}, err: {}, bytes: {:?}", msg.topic, err, msg.payload);
                        continue;
                    },
                };

                // coerce data into a number
                let mut send_data = Vec::new();
                for cap in num_regex.find_iter(&data) {
                    if let Ok(fnum) = cap.as_str().parse::<f32>() {
                        send_data.push(fnum);
                    }
                }

                if send_data.is_empty() {
                    warn!("Could not parse {}", topic);
                    continue;
                }

                let topic_sendable = topic.replace("$SYS", "SYS_tpu");

                let sendable = PublishableMessage { topic: topic_sendable, data: send_data, unit: "", time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64 };
                if let Err(err) = mqtt_send_tx.send(sendable).await {
                    warn!("Could not send SYS message out: {}", err);
                    continue;
                }
            }
        }
    }
}
