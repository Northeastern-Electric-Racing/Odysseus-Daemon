use std::process::Stdio;
use std::time::UNIX_EPOCH;

use crate::PublishableMessage;
use tokio::{io::AsyncBufReadExt, sync::mpsc::Sender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

pub async fn can_data_scraper(
    cancel_token: CancellationToken,
    mqtt_sender_tx: Sender<PublishableMessage>,
    base_name: String,
    can_iface: String,
) {
    let mut proc = tokio::process::Command::new("canbusload");
    let proc = proc
        .args([format!("{can_iface}@500000")])
        .stdout(Stdio::piped());
    let mut child = match proc.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("Could not create canbusload command: {}, exiting", e);
            return;
        }
    };

    let reader = tokio::io::BufReader::new(
        child
            .stdout
            .take()
            .expect("Could not read stdout of canbusload"),
    );
    let mut stream = reader.lines();

    let frames_topic = format!("{base_name}/Can/Frames");
    let bits_topic = format!("{base_name}/Can/Bits");
    let util_topic = format!("{base_name}/Can/Bus");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down MQTT processor!");
                break;
            },
            Ok(Some(line)) = stream.next_line() => {
                trace!("Got data from canbusload: {line}");
                if !line.is_empty() {
                    let sends = get_stats( line, &frames_topic, &bits_topic, &util_topic).await;
                    handle_sends(sends, &mqtt_sender_tx).await;
                }
            }
        }
    }
}

async fn get_stats(
    line: String,
    frames_topic: &str,
    bits_topic: &str,
    util_topc: &str,
) -> Vec<PublishableMessage> {
    let mut ret = vec![];

    let parts: Vec<_> = line.split_whitespace().collect();
    if let Some(res) = parts.get(1)
        && let Ok(fl) = res.parse::<f32>()
    {
        ret.push(PublishableMessage {
            topic: frames_topic.to_string(),
            data: vec![fl],
            unit: "frames/s",
            time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
        });
    }
    if let Some(res) = parts.get(2)
        && let Ok(fl) = res.parse::<f32>()
    {
        ret.push(PublishableMessage {
            topic: bits_topic.to_string(),
            data: vec![fl],
            unit: "bits/s",
            time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
        });
    }
    if let Some(res) = parts.last() {
        let res = res.trim_end_matches("%");
        if let Ok(fl) = res.parse::<f32>() {
            ret.push(PublishableMessage {
                topic: util_topc.to_string(),
                data: vec![fl / 100f32],
                unit: "%",
                time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
            });
        }
    }

    ret
}

async fn handle_sends(
    send_list: Vec<PublishableMessage>,
    mqtt_sender_tx: &Sender<PublishableMessage>,
) {
    for item in send_list.into_iter() {
        if let Err(err) = mqtt_sender_tx.send(item).await {
            warn!("Error putting message in MQTT queue for net: {} ", err);
        }
    }
}
