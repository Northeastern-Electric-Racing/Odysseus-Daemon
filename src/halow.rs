use std::time::Duration;
use std::time::UNIX_EPOCH;

use crate::PublishableMessage;
use regex::Regex;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::warn;

pub async fn halow_scraper(
    cancel_token: CancellationToken,
    base_name: String,
    mqtt_sender_tx: Sender<PublishableMessage>,
) {
    let mut signal_sync = tokio::time::interval(Duration::from_millis(50));

    let rssi_topic = format!("{base_name}/HaLow/RSSI");
    let mcs_topic_tx = format!("{base_name}/HaLow/TxMCS");
    let mcs_topic_rx = format!("{base_name}/HaLow/RxMCS");

    let rssi_regex = Regex::new(r"rssi\s*:\s*(-?\d+)").expect("Invalid halow regex rssi");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                debug!("Shutting down color controller!");
                break;
            },
            _ =  signal_sync.tick() => {
                let msgs = handle_tick(&rssi_regex, rssi_topic.clone(), mcs_topic_tx.clone(), mcs_topic_rx.clone()).await;
                handle_sends(msgs, &mqtt_sender_tx).await;
            }
        }
    }
}

async fn handle_tick(
    rssi_regex: &Regex,
    rssi_topic: String,
    mcs_topic_tx: String,
    mcs_topic_rx: String,
) -> Vec<PublishableMessage> {
    let mut send: Vec<PublishableMessage> = Vec::with_capacity(2);
    let output = Command::new("cli_app")
        .args(["show", "signal"])
        .output()
        .await
        .expect("Failed to run cli_app for halow");
    if let Ok(st) = str::from_utf8(&output.stdout)
        && let Some(caps) = rssi_regex.captures(st)
        && let Ok(fl) = caps[1].trim().parse::<f32>()
    {
        send.push(PublishableMessage {
            topic: rssi_topic.clone(),
            data: vec![fl],
            unit: "dBm",
            time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
        })
    } else {
        warn!(
            "Could not get RSSI from {:?}",
            str::from_utf8(&output.stdout)
        );
    }

    let output = Command::new("cli_app")
        .args(["show", "ap", "0"])
        .output()
        .await
        .expect("Failed to run cli_app for halow");

    if let Ok(st) = str::from_utf8(&output.stdout)
        && let Some(res) = st.lines().nth(2)
    {
        if let Some(l) = res.split_ascii_whitespace().nth(5)
            && let Some(r) = l.chars().rev().nth(1)
            && let Ok(res) = r.to_string().parse::<f32>()
        {
            send.push(PublishableMessage {
                topic: mcs_topic_rx.clone(),
                data: vec![res],
                unit: "",
                time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
            })
        }
        if let Some(l) = res.split_ascii_whitespace().nth(3)
            && let Some(r) = l.chars().rev().nth(1)
            && let Ok(res) = r.to_string().parse::<f32>()
        {
            send.push(PublishableMessage {
                topic: mcs_topic_tx.to_string(),
                data: vec![res],
                unit: "",
                time: UNIX_EPOCH.elapsed().unwrap().as_micros() as u64,
            })
        }
    }

    send
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
